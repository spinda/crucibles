#![cfg_attr(feature = "strict", deny(warnings))]
#![cfg_attr(feature = "strict", deny(missing_docs, missing_debug_implementations))]
#![cfg_attr(feature = "clippy", feature(plugin))]
#![cfg_attr(feature = "clippy", plugin(clippy))]
#![doc(html_root_url = "https://docs.rs/crucibles/0.1.1")]

extern crate futures;

use futures::{Async, AsyncSink, Future, IntoFuture, Poll, Sink, StartSend, Stream};
use futures::future::{self, Empty, ExecuteError, Executor};
use futures::stream::FuturesUnordered;
use futures::sync::mpsc;
use futures::task::{self, Task};
use std::cell::{RefCell, RefMut};
use std::fmt;
use std::rc::{Rc, Weak};

type SpawnFuture = Box<Future<Item = (), Error = ()>>;

struct CrucibleSpawned {
    futures: FuturesUnordered<SpawnFuture>,
    poll_task: Option<Task>,
    needs_notify: bool,
}

impl fmt::Debug for CrucibleSpawned {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("CrucibleSpawned")
            .field("futures", &Omitted)
            .field("poll_task", &self.poll_task)
            .field("needs_notify", &self.needs_notify)
            .finish()
    }
}

impl CrucibleSpawned {
    fn new() -> Self {
        CrucibleSpawned {
            futures: FuturesUnordered::new(),
            poll_task: None,
            needs_notify: false,
        }
    }

    fn poll(&mut self) {
        self.needs_notify = false;
        self.poll_task = Some(task::current());

        loop {
            match self.futures.poll() {
                Ok(Async::NotReady) | Ok(Async::Ready(None)) => break,
                Ok(Async::Ready(Some(_))) | Err(_) => (),
            }
        }
    }

    fn push(&mut self, future: SpawnFuture) {
        self.futures.push(future);
        self.needs_notify = true;
    }

    fn notify(&mut self) {
        if let Some(ref poll_task) = self.poll_task {
            poll_task.notify();
            self.needs_notify = false;
        }
    }

    fn notify_if_needed(&mut self) {
        if self.needs_notify {
            self.notify();
        }
    }

    fn spawn(&mut self, future: SpawnFuture) {
        self.push(future);
        self.notify();
    }
}

#[derive(Debug)]
struct CrucibleShared {
    spawned: RefCell<CrucibleSpawned>,
    remote: CrucibleRemote,
}

impl CrucibleShared {
    fn new(remote: CrucibleRemote) -> Self {
        CrucibleShared {
            spawned: RefCell::new(CrucibleSpawned::new()),
            remote: remote,
        }
    }

    fn borrow_spawned(&self) -> RefMut<CrucibleSpawned> {
        self.spawned.borrow_mut()
    }

    fn push(&self, future: SpawnFuture) {
        self.spawned.borrow_mut().push(future);
    }

    fn notify_if_needed(&self) {
        self.spawned.borrow_mut().notify_if_needed();
    }

    fn spawn(&self, future: SpawnFuture) {
        self.spawned.borrow_mut().spawn(future);
    }

    fn remote(&self) -> CrucibleRemote {
        self.remote.clone()
    }
}

trait FnBox: Send + 'static {
    fn call_box(self: Box<Self>, handle: CrucibleHandle) -> SpawnFuture;
}

impl<F> FnBox for F
where
    F: FnOnce(CrucibleHandle) -> SpawnFuture + Send + 'static,
{
    fn call_box(self: Box<Self>, handle: CrucibleHandle) -> SpawnFuture {
        (*self)(handle)
    }
}

enum RemoteMessage {
    SpawnFuture(Box<Future<Item = (), Error = ()> + Send>),
    SpawnFn(Box<FnBox>),
}

pub struct Crucible<T = Empty<(), ()>> {
    running: T,
    shared: Rc<CrucibleShared>,
    remote_receiver: mpsc::UnboundedReceiver<RemoteMessage>,
}

impl<T> fmt::Debug for Crucible<T>
where
    T: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Crucible")
            .field("running", &self.running)
            .field("shared", &self.shared)
            .field("remote_receiver", &Omitted)
            .finish()
    }
}

impl Crucible {
    pub fn new() -> Self {
        Crucible::run(future::empty())
    }
}

impl<T> Crucible<T> {
    pub fn run(future: T) -> Self {
        let (remote_sender, remote_receiver) = mpsc::unbounded();
        let remote = CrucibleRemote::new(remote_sender);
        Crucible {
            running: future,
            shared: Rc::new(CrucibleShared::new(remote)),
            remote_receiver: remote_receiver,
        }
    }

    pub fn spawn<F>(&self, future: F)
    where
        F: Future<Item = (), Error = ()> + 'static,
    {
        self.shared.spawn(Box::new(future));
    }

    pub fn spawn_fn<F, R>(&self, f: F)
    where
        F: FnOnce() -> R + 'static,
        R: IntoFuture<Item = (), Error = ()> + 'static,
    {
        self.spawn(future::lazy(f));
    }

    pub fn handle(&self) -> CrucibleHandle {
        CrucibleHandle::new(Rc::downgrade(&self.shared))
    }

    pub fn remote(&self) -> CrucibleRemote {
        self.shared.remote()
    }
}

impl<F> Executor<F> for Crucible
where
    F: Future<Item = (), Error = ()> + 'static,
{
    fn execute(&self, future: F) -> Result<(), ExecuteError<F>> {
        self.spawn(future);
        Ok(())
    }
}

impl<T> Future for Crucible<T>
where
    T: Future,
{
    type Item = T::Item;
    type Error = T::Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let mut spawned = self.shared.borrow_spawned();

        while let Ok(Async::Ready(Some(msg))) = self.remote_receiver.poll() {
            match msg {
                RemoteMessage::SpawnFuture(future) => spawned.push(future),
                RemoteMessage::SpawnFn(f) => spawned.push(f.call_box(self.handle())),
            }
        }

        spawned.poll();

        self.running.poll()
    }
}

impl<T> Sink for Crucible<T> {
    type SinkItem = Box<Future<Item = (), Error = ()>>;
    type SinkError = ();

    fn start_send(&mut self, item: Self::SinkItem) -> StartSend<Self::SinkItem, Self::SinkError> {
        self.shared.push(item);
        Ok(AsyncSink::Ready)
    }

    fn poll_complete(&mut self) -> Poll<(), Self::SinkError> {
        self.shared.notify_if_needed();
        Ok(().into())
    }

    fn close(&mut self) -> Poll<(), Self::SinkError> {
        self.poll_complete()
    }
}

#[derive(Clone, Debug)]
pub struct CrucibleHandle {
    shared: Weak<CrucibleShared>,
}

impl CrucibleHandle {
    fn new(shared: Weak<CrucibleShared>) -> Self {
        CrucibleHandle { shared: shared }
    }

    pub fn remote(&self) -> CrucibleRemote {
        match self.shared.upgrade() {
            Some(shared) => shared.remote(),
            None => CrucibleRemote::empty(),
        }
    }

    pub fn spawn<F>(&self, future: F)
    where
        F: Future<Item = (), Error = ()> + 'static,
    {
        if let Some(shared) = self.shared.upgrade() {
            shared.spawn(Box::new(future));
        }
    }

    pub fn spawn_fn<F, R>(&self, f: F)
    where
        F: FnOnce() -> R + 'static,
        R: IntoFuture<Item = (), Error = ()> + 'static,
    {
        self.spawn(future::lazy(f));
    }
}

impl<F> Executor<F> for CrucibleHandle
where
    F: Future<Item = (), Error = ()> + 'static,
{
    fn execute(&self, future: F) -> Result<(), ExecuteError<F>> {
        self.spawn(future);
        Ok(())
    }
}

impl Sink for CrucibleHandle {
    type SinkItem = Box<Future<Item = (), Error = ()>>;
    type SinkError = ();

    fn start_send(&mut self, item: Self::SinkItem) -> StartSend<Self::SinkItem, Self::SinkError> {
        if let Some(shared) = self.shared.upgrade() {
            shared.push(item);
        }
        Ok(AsyncSink::Ready)
    }

    fn poll_complete(&mut self) -> Poll<(), Self::SinkError> {
        if let Some(shared) = self.shared.upgrade() {
            shared.notify_if_needed();
        }
        Ok(().into())
    }

    fn close(&mut self) -> Poll<(), Self::SinkError> {
        self.poll_complete()
    }
}

#[derive(Clone)]
pub struct CrucibleRemote {
    maybe_remote_sender: Option<mpsc::UnboundedSender<RemoteMessage>>,
}

impl fmt::Debug for CrucibleRemote {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("CrucibleRemote")
            .field("maybe_remote_sender", &self.maybe_remote_sender.as_ref().map(|_| Omitted))
            .finish()
    }
}

impl CrucibleRemote {
    fn new(remote_sender: mpsc::UnboundedSender<RemoteMessage>) -> Self {
        CrucibleRemote {
            maybe_remote_sender: Some(remote_sender),
        }
    }

    fn empty() -> Self {
        CrucibleRemote {
            maybe_remote_sender: None,
        }
    }

    fn send(&self, msg: RemoteMessage) {
        if let Some(ref remote_sender) = self.maybe_remote_sender {
            let _ = remote_sender.unbounded_send(msg);
        }
    }

    pub fn spawn<F>(&self, future: F)
    where
        F: Future<Item = (), Error = ()> + Send + 'static,
    {
        self.send(RemoteMessage::SpawnFuture(Box::new(future)));
    }

    pub fn spawn_fn<F, R>(&self, f: F)
    where
        F: FnOnce(CrucibleHandle) -> R + Send + 'static,
        R: IntoFuture<Item = (), Error = ()>,
        R::Future: 'static,
    {
        self.send(RemoteMessage::SpawnFn(Box::new(move |handle| {
            let future: SpawnFuture = Box::new(f(handle).into_future());
            future
        })));
    }
}

impl<F> Executor<F> for CrucibleRemote
where
    F: Future<Item = (), Error = ()> + Send + 'static,
{
    fn execute(&self, future: F) -> Result<(), ExecuteError<F>> {
        self.spawn(future);
        Ok(())
    }
}

impl Sink for CrucibleRemote {
    type SinkItem = Box<Future<Item = (), Error = ()> + Send>;
    type SinkError = ();

    fn start_send(&mut self, item: Self::SinkItem) -> StartSend<Self::SinkItem, Self::SinkError> {
        match self.maybe_remote_sender {
            None => Ok(AsyncSink::Ready),
            Some(ref mut remote_sender) => {
                match remote_sender.start_send(RemoteMessage::SpawnFuture(item)) {
                    Ok(AsyncSink::Ready) | Err(_) => Ok(AsyncSink::Ready),
                    Ok(AsyncSink::NotReady(msg)) => match msg {
                        RemoteMessage::SpawnFuture(item) => Ok(AsyncSink::NotReady(item)),
                        RemoteMessage::SpawnFn(_) => unreachable!(
                            "crucibles: UnboundedSender::start_send violated Sink contract"
                        ),
                    },
                }
            }
        }
    }

    fn poll_complete(&mut self) -> Poll<(), Self::SinkError> {
        match self.maybe_remote_sender {
            None => Ok(().into()),
            Some(ref mut remote_sender) => match remote_sender.poll_complete() {
                Ok(Async::Ready(_)) | Err(_) => Ok(().into()),
                Ok(Async::NotReady) => Ok(Async::NotReady),
            },
        }
    }

    fn close(&mut self) -> Poll<(), Self::SinkError> {
        self.poll_complete()
    }
}

struct Omitted;

impl fmt::Debug for Omitted {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "...")
    }
}
