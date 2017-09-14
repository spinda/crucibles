extern crate futures;
extern crate tokio_core;

extern crate crucibles;

use crucibles::Crucible;
use futures::{Future, Sink};
use futures::future;
use futures::sync::oneshot;
use std::io;
use std::thread;
use tokio_core::reactor::Core;

#[test]
fn test_crucible() {
    let mut core = Core::new().expect("core creation error");
    let crucible = Crucible::run(future::ok::<_, io::Error>(true));
    assert_eq!(true, core.run(crucible).expect("core run error"));
}

#[test]
fn test_crucible_complete() {
    let mut core = Core::new().expect("core creation error");
    let (sender, receiver) = oneshot::channel();
    let crucible = Crucible::run(receiver);
    crucible.spawn(future::lazy(move || {
        sender.send(true).expect("send error");
        Ok(())
    }));
    assert_eq!(true, core.run(crucible).expect("core run error"));
}

#[test]
fn test_crucible_sink_complete() {
    let mut core = Core::new().expect("core creation error");
    let (sender, receiver) = oneshot::channel();
    let crucible = Crucible::run(receiver);
    let crucible = core.run(crucible.send(Box::new(future::lazy(move || {
        sender.send(true).expect("send error");
        Ok(())
    })))).expect("sink send error");
    assert_eq!(true, core.run(crucible).expect("core run error"));
}

#[test]
fn test_crucible_interrupt() {
    let mut core = Core::new().expect("core creation error");
    let crucible = Crucible::run(future::ok::<_, io::Error>(true));
    crucible.spawn(future::empty());
    assert_eq!(true, core.run(crucible).expect("core run error"));
}

#[test]
fn test_crucible_sink_interrupt() {
    let mut core = Core::new().expect("core creation error");
    let crucible = Crucible::run(future::ok::<_, io::Error>(true));
    let crucible = core.run(crucible.send(Box::new(future::empty()))).expect("sink send error");
    assert_eq!(true, core.run(crucible).expect("core run error"));
}

#[test]
fn test_crucible_handle_complete() {
    let mut core = Core::new().expect("core creation error");
    let (sender, receiver) = oneshot::channel();
    let crucible = Crucible::run(receiver);
    crucible.handle().spawn(future::lazy(move || {
        sender.send(true).expect("send error");
        Ok(())
    }));
    assert_eq!(true, core.run(crucible).expect("core run error"));
}

#[test]
fn test_crucible_handle_sink_complete() {
    let mut core = Core::new().expect("core creation error");
    let (sender, receiver) = oneshot::channel();
    let crucible = Crucible::run(receiver);
    core.run(crucible.handle().send(Box::new(future::lazy(move || {
        sender.send(true).expect("send error");
        Ok(())
    })))).expect("sink send error");
    assert_eq!(true, core.run(crucible).expect("core run error"));
}

#[test]
fn test_crucible_handle_interrupt() {
    let mut core = Core::new().expect("core creation error");
    let crucible = Crucible::run(future::ok::<_, io::Error>(true));
    crucible.handle().spawn(future::empty());
    assert_eq!(true, core.run(crucible).expect("core run error"));
}

#[test]
fn test_crucible_handle_sink_interrupt() {
    let mut core = Core::new().expect("core creation error");
    let crucible = Crucible::run(future::ok::<_, io::Error>(true));
    core.run(crucible.handle().send(Box::new(future::empty()))).expect("sink send error");
    assert_eq!(true, core.run(crucible).expect("core run error"));
}

#[test]
fn test_crucible_remote_complete() {
    let mut core = Core::new().expect("core creation error");
    let (sender, receiver) = oneshot::channel();
    let crucible = Crucible::run(receiver);
    let remote = crucible.remote();
    thread::spawn(move || {
        remote.spawn(future::lazy(move || {
            sender.send(true).expect("send error");
            Ok(())
        }));
    });
    assert_eq!(true, core.run(crucible).expect("core run error"));
}

#[test]
fn test_crucible_remote_sink_complete() {
    let mut core = Core::new().expect("core creation error");
    let (thread_sender, thread_receiver) = oneshot::channel();
    let (sender, receiver) = oneshot::channel();
    let crucible = Crucible::run(future::lazy(move || {
        thread_sender.send(()).expect("send error");
        receiver
    }));
    let remote = crucible.remote();
    thread::spawn(move || {
        let mut core = Core::new().expect("core creation error");
        core.run(thread_receiver.then(|result| {
            result.expect("receive error");
            remote.send(Box::new(future::lazy(move || {
                sender.send(true).expect("send error");
                Ok(())
            })))
        })).expect("sink send error");
    });
    assert_eq!(true, core.run(crucible).expect("core run error"));
}

#[test]
fn test_crucible_remote_interrupt() {
    let mut core = Core::new().expect("core creation error");
    let crucible = Crucible::run(future::ok::<_, io::Error>(true));
    let remote = crucible.remote();
    thread::spawn(move || {
        remote.spawn(future::empty());
    });
    assert_eq!(true, core.run(crucible).expect("core run error"));
}

#[test]
fn test_crucible_remote_sink_interrupt() {
    let mut core = Core::new().expect("core creation error");
    let crucible = Crucible::run(future::ok::<_, io::Error>(true));
    let remote = crucible.remote();
    thread::spawn(move || {
        let mut core = Core::new().expect("core creation error");
        core.run(remote.send(Box::new(future::empty()))).expect("sink send error");
    });
    assert_eq!(true, core.run(crucible).expect("core run error"));
}

#[test]
fn test_crucible_new_compiles() {
    let _crucible = Crucible::new();
}
