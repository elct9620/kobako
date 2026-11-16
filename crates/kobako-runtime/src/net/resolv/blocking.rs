//! The system's `getaddrinfo` on a thread of its own, for a host whose
//! cancellable query is not yet built.

use std::future::poll_fn;
use std::io;
use std::net::{IpAddr, ToSocketAddrs};
use std::sync::{Arc, Mutex, PoisonError};
use std::task::{Poll, Waker};
use std::thread;

use crate::net::lookup::Resolving;

type Answer = (Option<io::Result<Vec<IpAddr>>>, Option<Waker>);

pub(super) fn resolve(name: &str) -> Resolving {
    let name = name.to_string();
    let shared = Arc::new(Mutex::new(Answer::default()));
    let answering = shared.clone();
    let spawned = thread::Builder::new()
        .name("kobako-resolver".into())
        .spawn(move || {
            let found = (name.as_str(), 0).to_socket_addrs().map(|addrs| {
                let mut addresses = Vec::new();
                for addr in addrs {
                    if !addresses.contains(&addr.ip()) {
                        addresses.push(addr.ip());
                    }
                }
                addresses
            });
            let mut answer = answering.lock().unwrap_or_else(PoisonError::into_inner);
            answer.0 = Some(found);
            if let Some(waker) = answer.1.take() {
                waker.wake();
            }
        });
    if let Err(err) = spawned {
        return Box::pin(std::future::ready(Err(err)));
    }
    Box::pin(poll_fn(move |cx| {
        let mut answer = shared.lock().unwrap_or_else(PoisonError::into_inner);
        match answer.0.take() {
            Some(found) => Poll::Ready(found),
            None => {
                answer.1 = Some(cx.waker().clone());
                Poll::Pending
            }
        }
    }))
}
