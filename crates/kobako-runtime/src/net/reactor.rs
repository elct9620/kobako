//! The one thread that waits on every host socket of the process, as Linux's
//! softirq hands each arriving segment to the socket it belongs to: an event
//! calls its socket's `on_event`, which moves what it can without blocking and
//! wakes the socket's wait queue. The same thread runs the tasks an HTTP
//! exchange needs, so no socket holds a thread of its own. A forked child,
//! which inherits no thread, starts a reactor of its own.

use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::io;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, LazyLock, Mutex, PoisonError, Weak};
use std::task::{Context, Wake, Waker};
use std::thread;

use mio::event::Source;
use mio::{Events, Interest, Registry, Token};

/// What a host socket does once the reactor saw it change, as a socket's
/// `sk_data_ready` and `sk_write_space` do: move what it can without blocking,
/// then wake whoever waits on it.
pub(crate) trait Ready: Send + Sync {
    fn on_event(&self);
}

/// The token that wakes the reactor to run the tasks queued for it.
const RUN_TASKS: Token = Token(0);

pub(crate) struct Reactor {
    registry: Registry,
    waker: mio::Waker,
    next: AtomicUsize,
    /// The socket each token belongs to, held weakly so the reactor keeps no
    /// socket alive past its release.
    sockets: Mutex<HashMap<Token, Weak<dyn Ready>>>,
    queued: Mutex<VecDeque<Arc<Task>>>,
    /// Whether the thread stopped, its poll having failed, so the next socket
    /// starts another reactor rather than register on one nobody waits on.
    stopped: AtomicBool,
}

/// A reactor, and the process it was started in.
type StartedReactor = (u32, Arc<Reactor>);

/// The process's reactor.
static CURRENT: LazyLock<Mutex<Option<StartedReactor>>> = LazyLock::new(Mutex::default);

impl Reactor {
    /// The process's reactor, started at the first socket; a forked child
    /// starts its own, since its parent's thread never came along, and so does
    /// the socket after a reactor whose poll failed.
    pub(crate) fn current() -> io::Result<Arc<Self>> {
        let mut current = CURRENT.lock().unwrap_or_else(PoisonError::into_inner);
        let pid = std::process::id();
        if let Some((started_in, reactor)) = current.as_ref() {
            if *started_in == pid && !reactor.stopped.load(Ordering::Acquire) {
                return Ok(reactor.clone());
            }
        }
        let reactor = Self::start()?;
        *current = Some((pid, reactor.clone()));
        Ok(reactor)
    }

    fn start() -> io::Result<Arc<Self>> {
        let mut poll = mio::Poll::new()?;
        let reactor = Arc::new(Self {
            registry: poll.registry().try_clone()?,
            waker: mio::Waker::new(poll.registry(), RUN_TASKS)?,
            next: AtomicUsize::new(1),
            sockets: Mutex::default(),
            queued: Mutex::default(),
            stopped: AtomicBool::new(false),
        });
        let running = reactor.clone();
        thread::Builder::new()
            .name("kobako-reactor".into())
            .spawn(move || running.run(&mut poll))?;
        Ok(reactor)
    }

    fn run(&self, poll: &mut mio::Poll) {
        let mut events = Events::with_capacity(256);
        loop {
            if let Err(err) = poll.poll(&mut events, None) {
                if err.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                self.stopped.store(true, Ordering::Release);
                return;
            }
            let mut run_tasks = false;
            let ready: Vec<Arc<dyn Ready>> = {
                let sockets = self.sockets.lock().unwrap_or_else(PoisonError::into_inner);
                events
                    .iter()
                    .filter_map(|event| {
                        if event.token() == RUN_TASKS {
                            run_tasks = true;
                            return None;
                        }
                        sockets.get(&event.token()).and_then(Weak::upgrade)
                    })
                    .collect()
            };
            ready.iter().for_each(|socket| socket.on_event());
            if run_tasks {
                self.run_tasks();
            }
        }
    }

    /// Watch `source` for reading and writing, handing each change to `ready`'s
    /// `on_event`.
    pub(crate) fn register(
        self: &Arc<Self>,
        source: &mut impl Source,
        ready: Weak<dyn Ready>,
    ) -> io::Result<Registration> {
        let token = Token(self.next.fetch_add(1, Ordering::Relaxed));
        self.sockets
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(token, ready);
        if let Err(err) =
            self.registry
                .register(source, token, Interest::READABLE | Interest::WRITABLE)
        {
            self.forget(token);
            return Err(err);
        }
        Ok(Registration {
            token,
            reactor: self.clone(),
        })
    }

    fn forget(&self, token: Token) {
        self.sockets
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&token);
    }

    /// Run `future` on the reactor until it ends or the task is cancelled.
    pub(crate) fn spawn(
        self: &Arc<Self>,
        future: impl Future<Output = ()> + Send + 'static,
    ) -> Arc<Task> {
        let task = Arc::new(Task {
            future: Mutex::new(Some(Box::pin(future))),
            reactor: Arc::downgrade(self),
            queued: AtomicBool::new(false),
        });
        task.clone().wake();
        task
    }

    fn run_tasks(&self) {
        loop {
            let Some(task) = self
                .queued
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .pop_front()
            else {
                return;
            };
            task.queued.store(false, Ordering::Release);
            let waker = Waker::from(task.clone());
            let mut future = task.future.lock().unwrap_or_else(PoisonError::into_inner);
            let ended = future.as_mut().is_some_and(|running| {
                running
                    .as_mut()
                    .poll(&mut Context::from_waker(&waker))
                    .is_ready()
            });
            if ended {
                *future = None;
            }
        }
    }
}

/// One host socket the reactor watches; the socket deregisters it as it
/// lets the host socket go.
pub(crate) struct Registration {
    token: Token,
    reactor: Arc<Reactor>,
}

impl Registration {
    /// Stop watching `source`, which the socket closes next; the token goes
    /// once the registration drops.
    pub(crate) fn deregister(&self, source: &mut impl Source) {
        let _ = self.reactor.registry.deregister(source);
    }
}

impl Drop for Registration {
    fn drop(&mut self) {
        self.reactor.forget(self.token);
    }
}

/// A future the reactor runs, as an executor's task.
pub(crate) struct Task {
    future: Mutex<Option<Pin<Box<dyn Future<Output = ()> + Send>>>>,
    reactor: Weak<Reactor>,
    queued: AtomicBool,
}

impl Task {
    /// Drop the future where it stands, with every socket it holds.
    pub(crate) fn cancel(&self) {
        let future = self
            .future
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        drop(future);
    }
}

impl Wake for Task {
    fn wake(self: Arc<Self>) {
        let Some(reactor) = self.reactor.upgrade() else {
            return;
        };
        if self.queued.swap(true, Ordering::AcqRel) {
            return;
        }
        reactor
            .queued
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push_back(self);
        let _ = reactor.waker.wake();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::task::Poll;
    use std::time::Duration;

    use super::*;

    // A reactor started in another process, as a forked child inherits its
    // parent's, is never handed out: the child starts its own.
    #[test]
    fn a_reactor_started_in_another_process_is_replaced() {
        let inherited = Reactor::current().expect("a reactor starts");
        CURRENT.lock().unwrap().as_mut().unwrap().0 = std::process::id().wrapping_add(1);
        let own = Reactor::current().expect("a reactor starts");
        assert!(
            !Arc::ptr_eq(&inherited, &own),
            "Reactor::current must start a reactor of its own once the one it holds was started \
             in another process"
        );
    }

    // A task woken from another thread runs on the reactor and ends.
    #[test]
    fn a_spawned_task_runs_to_its_end_once_woken() {
        let reactor = Reactor::current().expect("a reactor starts");
        let (tx, rx) = mpsc::channel();
        let gate = Arc::new(Mutex::new((false, None::<Waker>)));
        let waiting = gate.clone();
        reactor.spawn(async move {
            std::future::poll_fn(|cx| {
                let mut gate = waiting.lock().unwrap();
                if gate.0 {
                    Poll::Ready(())
                } else {
                    gate.1 = Some(cx.waker().clone());
                    Poll::Pending
                }
            })
            .await;
            tx.send(()).unwrap();
        });
        thread::sleep(Duration::from_millis(20));
        let waker = {
            let mut gate = gate.lock().unwrap();
            gate.0 = true;
            gate.1.take()
        };
        waker.expect("the task waited").wake();
        assert!(
            rx.recv_timeout(Duration::from_secs(5)).is_ok(),
            "a task woken from another thread must run on the reactor to its end"
        );
    }
}
