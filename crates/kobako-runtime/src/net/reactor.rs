//! The one thread that waits on every host socket of the process, as Linux's
//! softirq hands each arriving segment to the socket it belongs to: an event
//! calls its socket's `on_event`, which moves what it can without blocking and
//! wakes the socket's wait queue. The same thread runs the tasks an HTTP
//! exchange and a name lookup need, and wakes a task whose timer is due, so
//! no socket holds a thread of its own. A forked child, which inherits no
//! thread, starts a reactor of its own.

use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::io;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError, Weak};
use std::task::{Context, Wake, Waker};
use std::thread;
use std::time::{Duration, Instant};

use mio::event::Source;
use mio::{Events, Interest, Registry, Token};

use crate::fork::ProcessLocal;

/// What a host socket does once the reactor saw it change, as a socket's
/// `sk_data_ready` and `sk_write_space` do: move what it can without blocking,
/// then wake whoever waits on it.
pub(crate) trait Ready: Send + Sync {
    fn on_event(&self);
}

/// The token that wakes the reactor to run the tasks queued for it.
const WAKE_TOKEN: Token = Token(0);

pub(crate) struct Reactor {
    registry: Registry,
    waker: mio::Waker,
    next: AtomicUsize,
    /// The socket each token belongs to, held weakly so the reactor keeps no
    /// socket alive past its release.
    sockets: Mutex<HashMap<Token, Weak<dyn Ready>>>,
    /// The tasks woken and waiting their turn, as a scheduler's run queue.
    run_queue: Mutex<VecDeque<Arc<Task>>>,
    /// The sockets whose last turn ended at its bound with more waiting, each
    /// once, run again after the next round of events, as NAPI puts a poll
    /// that spent its weight back at the tail of `poll_list`.
    repoll: Mutex<Vec<Weak<dyn Ready>>>,
    /// Whether the thread stopped, its poll having failed, so the next socket
    /// starts another reactor rather than register on one nobody waits on.
    stopped: AtomicBool,
    /// The wakers due at an instant, as hrtimers wake a task whose timeout
    /// ends.
    timers: Mutex<Vec<(Instant, Waker)>>,
}

/// The process's reactor.
static CURRENT: ProcessLocal<Arc<Reactor>> = ProcessLocal::new();

impl Reactor {
    /// The process's reactor, started at the first socket; a forked child
    /// starts its own, since its parent's thread never came along, and so does
    /// the socket after a reactor whose poll failed.
    pub(crate) fn current() -> io::Result<Arc<Self>> {
        CURRENT.with(|current| {
            if let Some(reactor) = current.as_ref() {
                if !reactor.stopped.load(Ordering::Acquire) {
                    return Ok(reactor.clone());
                }
            }
            let reactor = Self::start()?;
            *current = Some(reactor.clone());
            Ok(reactor)
        })
    }

    fn start() -> io::Result<Arc<Self>> {
        let mut poll = mio::Poll::new()?;
        let reactor = Arc::new(Self {
            registry: poll.registry().try_clone()?,
            waker: mio::Waker::new(poll.registry(), WAKE_TOKEN)?,
            next: AtomicUsize::new(1),
            sockets: Mutex::default(),
            run_queue: Mutex::default(),
            repoll: Mutex::default(),
            stopped: AtomicBool::new(false),
            timers: Mutex::default(),
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
            // What asked for another turn before this round runs after its
            // events, as `net_rx_action` takes the polls newly scheduled
            // ahead of its repoll list; what asks during the round waits for
            // the next.
            let again =
                std::mem::take(&mut *self.repoll.lock().unwrap_or_else(PoisonError::into_inner));
            let wait = match again.is_empty() {
                true => self.poll_timeout(),
                false => Some(Duration::ZERO),
            };
            if let Err(err) = poll.poll(&mut events, wait) {
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
                        if event.token() == WAKE_TOKEN {
                            run_tasks = true;
                            return None;
                        }
                        sockets.get(&event.token()).and_then(Weak::upgrade)
                    })
                    .collect()
            };
            ready.iter().for_each(|socket| socket.on_event());
            again
                .iter()
                .filter_map(Weak::upgrade)
                .for_each(|socket| socket.on_event());
            self.wake_due_timers();
            if run_tasks {
                self.run_tasks();
            }
        }
    }

    /// Wake `waker` once `at` passes, in place of any instant its task asked
    /// for before, as `hrtimer_start` moves a task's one timer. The timer
    /// lasts until it is due or its `Timer` drops, so a wait that ends or is
    /// cancelled leaves none behind.
    #[cfg(any(test, all(unix, not(target_os = "macos"))))]
    pub(crate) fn wake_at(self: &Arc<Self>, at: Instant, waker: Waker) -> Timer {
        self.disarm(&waker);
        self.timers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push((at, waker.clone()));
        let _ = self.waker.wake();
        Timer {
            reactor: self.clone(),
            waker,
        }
    }

    /// Drop the timer `waker`'s task holds, as `hrtimer_cancel` does.
    #[cfg(any(test, all(unix, not(target_os = "macos"))))]
    fn disarm(&self, waker: &Waker) {
        self.timers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .retain(|(_, armed)| !armed.will_wake(waker));
    }

    /// How long the poll may wait before the soonest timer is due.
    fn poll_timeout(&self) -> Option<Duration> {
        let timers = self.timers.lock().unwrap_or_else(PoisonError::into_inner);
        let soonest = timers.iter().map(|(at, _)| *at).min()?;
        Some(soonest.saturating_duration_since(Instant::now()))
    }

    fn wake_due_timers(&self) {
        let now = Instant::now();
        let due: Vec<Waker> = {
            let mut timers = self.timers.lock().unwrap_or_else(PoisonError::into_inner);
            let (due, pending) = std::mem::take(&mut *timers)
                .into_iter()
                .partition(|(at, _)| *at <= now);
            *timers = pending;
            due.into_iter().map(|(_, waker)| waker).collect()
        };
        due.into_iter().for_each(Waker::wake);
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
                .run_queue
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

    /// Run `ready`'s `on_event` again after the reactor's next round of
    /// events, once however often it asks, as NAPI repolls a device that
    /// spent its weight with more to take and `NAPI_STATE_SCHED` keeps it
    /// queued once; no further event may come for what already waits.
    pub(crate) fn repoll(&self, ready: Weak<dyn Ready>) {
        let mut repoll = self
            .reactor
            .repoll
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if !repoll.iter().any(|queued| queued.ptr_eq(&ready)) {
            repoll.push(ready);
        }
        drop(repoll);
        let _ = self.reactor.waker.wake();
    }
}

impl Drop for Registration {
    fn drop(&mut self) {
        self.reactor.forget(self.token);
    }
}

/// A task's armed timer, disarmed as it drops.
#[cfg(any(test, all(unix, not(target_os = "macos"))))]
pub(crate) struct Timer {
    reactor: Arc<Reactor>,
    waker: Waker,
}

#[cfg(any(test, all(unix, not(target_os = "macos"))))]
impl Drop for Timer {
    fn drop(&mut self) {
        self.reactor.disarm(&self.waker);
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
            .run_queue
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
        CURRENT.inherit();
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

    // A task that asks to be woken at an instant runs again once it passes,
    // with nothing else waking it.
    #[test]
    fn a_timer_wakes_its_task_once_due() {
        let reactor = Reactor::current().expect("a reactor starts");
        let (tx, rx) = mpsc::channel();
        let due = Instant::now() + Duration::from_millis(50);
        let timing = reactor.clone();
        reactor.spawn(async move {
            let mut timer = None;
            std::future::poll_fn(|cx| {
                if Instant::now() >= due {
                    return Poll::Ready(());
                }
                timer.get_or_insert_with(|| timing.wake_at(due, cx.waker().clone()));
                Poll::Pending
            })
            .await;
            tx.send(()).unwrap();
        });
        assert!(
            rx.recv_timeout(Duration::from_secs(5)).is_ok(),
            "a task the reactor was asked to wake at an instant must run again once it passes"
        );
    }

    // A waker asked to be woken again holds one timer, the newest, however
    // often it asked before.
    #[test]
    fn a_waker_asking_again_holds_one_timer() {
        let reactor = Reactor::current().expect("a reactor starts");
        let waker = Waker::noop().clone();
        let later = Instant::now() + Duration::from_secs(30);
        let armed: Vec<Timer> = (0..3)
            .map(|_| reactor.wake_at(later, waker.clone()))
            .collect();
        let held = timers_of(&reactor, &waker);
        drop(armed);
        assert_eq!(
            (held, timers_of(&reactor, &waker)),
            (1, 0),
            "a waker asked to be woken three times must hold one timer, the newest, and none \
             once its timers drop"
        );
    }

    /// How many timers `reactor` holds for `waker`.
    fn timers_of(reactor: &Reactor, waker: &Waker) -> usize {
        reactor
            .timers
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, armed)| armed.will_wake(waker))
            .count()
    }

    // A task cancelled while its timer is armed leaves no timer behind, so a
    // guest dropping lookups never fills the reactor's timers.
    #[test]
    fn a_cancelled_task_leaves_no_timer() {
        let reactor = Reactor::current().expect("a reactor starts");
        let (armed_tx, armed_rx) = mpsc::channel();
        let timing = reactor.clone();
        let later = Instant::now() + Duration::from_secs(30);
        let task = reactor.spawn(async move {
            let mut timer = None;
            std::future::poll_fn(|cx| {
                timer.get_or_insert_with(|| timing.wake_at(later, cx.waker().clone()));
                let _ = armed_tx.send(());
                Poll::<()>::Pending
            })
            .await;
        });
        armed_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("the task arms its timer");
        let waker = Waker::from(task.clone());
        let held = timers_of(&reactor, &waker);
        task.cancel();
        assert_eq!(
            (held, timers_of(&reactor, &waker)),
            (1, 0),
            "a cancelled lookup's timer must leave the reactor with the lookup, not stay until due"
        );
    }
}
