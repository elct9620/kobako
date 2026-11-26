//! The one thread that waits on every host socket of the process, as Linux's
//! softirq hands each arriving segment to the socket it belongs to: an event
//! calls its socket's `on_event`, which moves what it can without blocking and
//! wakes the socket's wait queue. The same thread wakes a waker whose timer is
//! due, so no socket holds a thread of its own. It only wakes: an HTTP
//! exchange or a name lookup moves on the thread that waits for it. A forked
//! child, which inherits no thread, starts a reactor of its own.

use std::collections::HashMap;
use std::io;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};
use std::task::Waker;
use std::thread;
use std::time::{Duration, Instant};

use mio::event::Source;
use mio::{Events, Interest, Registry, Token};

use crate::fork::{Pid, ProcessLocal};
use crate::vfs::PollMask;

/// The poll mask `event` reports: a hang-up once both directions closed, as
/// a reset or two FINs leave a socket, which Windows' AFD reports alone.
fn poll_mask_of(event: &mio::event::Event) -> PollMask {
    let mut mask = PollMask::default();
    if event.is_readable() {
        mask = mask | PollMask::IN;
    }
    if event.is_writable() {
        mask = mask | PollMask::OUT;
    }
    if event.is_error() {
        mask = mask | PollMask::ERR;
    }
    if event.is_read_closed() && event.is_write_closed() {
        mask = mask | PollMask::HUP;
    }
    mask
}

/// What a host socket does once the reactor saw it change, as a socket's
/// `sk_data_ready` and `sk_write_space` do: move what it can without blocking,
/// then wake whoever waits on it. `events` is what the change reported, as a
/// wakeup's key carries the poll mask; a repoll reports nothing. A socket
/// that stops short of `WouldBlock` hears of the host socket again only
/// through `events`, a repoll, or its guest's next call.
pub(crate) trait Ready: Send + Sync {
    fn on_event(&self, events: PollMask);

    /// Fail the socket from outside, as the reactor stopping does, and wake
    /// whoever waits on it, as `tcp_abort` and `udp_abort` end one.
    fn abort(&self);
}

/// The token that wakes the reactor to read its timers and repolls again.
const WAKE_TOKEN: Token = Token(0);

pub(crate) struct Reactor {
    registry: Registry,
    waker: mio::Waker,
    next: AtomicUsize,
    /// The socket each token belongs to, held weakly so the reactor keeps no
    /// socket alive past its release.
    sockets: Mutex<HashMap<Token, Weak<dyn Ready>>>,
    /// The sockets whose last turn ended at its bound with more waiting, each
    /// once, run again after the next round of events, as NAPI puts a poll
    /// that spent its weight back at the tail of `poll_list`.
    repoll: Mutex<Vec<Weak<dyn Ready>>>,
    /// Whether the thread stopped, its poll having failed or a callback
    /// panicked, so the next socket starts another reactor rather than
    /// register on one nobody waits on.
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
    /// the socket after a reactor that stopped.
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
        let reactor = Self::on(&poll)?;
        let running = reactor.clone();
        thread::Builder::new()
            .name("kobako-reactor".into())
            .spawn(move || running.run(|events, wait| poll.poll(events, wait)))?;
        Ok(reactor)
    }

    /// A reactor registering on `poll`, whose events nobody reads yet.
    fn on(poll: &mio::Poll) -> io::Result<Arc<Self>> {
        Ok(Arc::new(Self {
            registry: poll.registry().try_clone()?,
            waker: mio::Waker::new(poll.registry(), WAKE_TOKEN)?,
            next: AtomicUsize::new(1),
            sockets: Mutex::default(),
            repoll: Mutex::default(),
            stopped: AtomicBool::new(false),
            timers: Mutex::default(),
        }))
    }

    /// Serve each round of the events `poll` waits for, until it fails.
    fn run(&self, mut poll: impl FnMut(&mut Events, Option<Duration>) -> io::Result<()>) {
        let _stopping = Stopping(self);
        let mut events = Events::with_capacity(256);
        loop {
            let wait = match self.repoll().is_empty() {
                true => self.poll_timeout(),
                false => Some(Duration::ZERO),
            };
            if let Err(err) = poll(&mut events, wait) {
                if err.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                return;
            }
            // What asked for another turn before this round runs after its
            // events, as `net_rx_action` takes the polls newly scheduled
            // ahead of its repoll list; what asks during the round waits for
            // the next. A poll a signal interrupted takes none, so each
            // waits on in place, as `poll_list` keeps its entries.
            let again = std::mem::take(&mut *self.repoll());
            let ready: Vec<(Arc<dyn Ready>, PollMask)> = {
                let sockets = self.sockets.lock().unwrap_or_else(PoisonError::into_inner);
                events
                    .iter()
                    .filter_map(|event| {
                        let socket = sockets.get(&event.token()).and_then(Weak::upgrade)?;
                        Some((socket, poll_mask_of(event)))
                    })
                    .collect()
            };
            ready
                .iter()
                .for_each(|(socket, events)| socket.on_event(*events));
            again
                .iter()
                .filter_map(Weak::upgrade)
                .for_each(|socket| socket.on_event(PollMask::default()));
            self.wake_due_timers();
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
        // A reactor that stopped never reads its timers again.
        if self.stopped.load(Ordering::Acquire) {
            self.disarm(&waker);
            waker.wake_by_ref();
        }
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

    fn repoll(&self) -> MutexGuard<'_, Vec<Weak<dyn Ready>>> {
        self.repoll.lock().unwrap_or_else(PoisonError::into_inner)
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
    /// `on_event`; a reactor that stopped refuses it with `ECONNABORTED`, as
    /// nobody would wake its socket.
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
        // Read after the token is in place, so a reactor stopping meanwhile
        // either aborts the socket or is seen here.
        if self.stopped.load(Ordering::Acquire) {
            let _ = self.registry.deregister(source);
            self.forget(token);
            return Err(io::ErrorKind::ConnectionAborted.into());
        }
        Ok(Registration {
            token,
            reactor: self.clone(),
            pid: Pid::current(),
        })
    }

    fn forget(&self, token: Token) {
        self.sockets
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&token);
    }
}

/// Stops the reactor as its thread leaves the loop, by a failed poll or a
/// panic alike: no socket registers on it again, and each socket and timer
/// it watched is aborted or woken, so no waiter waits on a thread that is
/// gone.
struct Stopping<'a>(&'a Reactor);

impl Drop for Stopping<'_> {
    fn drop(&mut self) {
        let reactor = self.0;
        reactor.stopped.store(true, Ordering::Release);
        let watched: Vec<Arc<dyn Ready>> = reactor
            .sockets
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .values()
            .filter_map(Weak::upgrade)
            .collect();
        watched.iter().for_each(|socket| socket.abort());
        let timers = std::mem::take(
            &mut *reactor
                .timers
                .lock()
                .unwrap_or_else(PoisonError::into_inner),
        );
        timers.into_iter().for_each(|(_, waker)| waker.wake());
    }
}

/// One host socket the reactor watches; the socket deregisters it as it
/// lets the host socket go.
pub(crate) struct Registration {
    token: Token,
    reactor: Arc<Reactor>,
    pid: Pid,
}

impl Registration {
    /// Stop watching `source`, which the socket closes next; the token goes
    /// once the registration drops. A forked child leaves its parent's
    /// registration be: the epoll instance is shared, and removing the one
    /// `<fd, file>` entry would stop the parent watching its own socket.
    pub(crate) fn deregister(&self, source: &mut impl Source) {
        if self.pid.is_current() {
            let _ = self.reactor.registry.deregister(source);
        }
    }

    /// Run `ready`'s `on_event` again after the reactor's next round of
    /// events, once however often it asks, as NAPI repolls a device that
    /// spent its weight with more to take and `NAPI_STATE_SCHED` keeps it
    /// queued once; no further event may come for what already waits.
    pub(crate) fn repoll(&self, ready: Weak<dyn Ready>) {
        let mut repoll = self.reactor.repoll();
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

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::task::{Poll, Wake};
    use std::time::Duration;

    use super::super::task::Task;
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

    /// A socket the reactor tells of each change through a channel.
    struct Tells(Mutex<mpsc::Sender<()>>);

    impl Ready for Tells {
        fn on_event(&self, _events: PollMask) {
            let _ = self.0.lock().unwrap().send(());
        }

        fn abort(&self) {}
    }

    /// A socket that tells only of its abort.
    struct TellsAbort(Mutex<mpsc::Sender<&'static str>>);

    impl Ready for TellsAbort {
        fn on_event(&self, _events: PollMask) {}

        fn abort(&self) {
            let _ = self.0.lock().unwrap().send("the socket");
        }
    }

    /// A socket whose callback fails once armed, as a panic in the code it
    /// runs would. Unarmed, it lets pass the event its registration brings,
    /// as a socket is writable the moment it registers.
    struct PanicsOnceArmed(AtomicBool);

    impl Ready for PanicsOnceArmed {
        fn on_event(&self, _events: PollMask) {
            if self.0.load(Ordering::Acquire) {
                panic!("a socket's callback failed on the reactor");
            }
        }

        fn abort(&self) {}
    }

    /// A timer's task that tells as it wakes.
    struct TellsWake(Mutex<mpsc::Sender<&'static str>>);

    impl Wake for TellsWake {
        fn wake(self: Arc<Self>) {
            let _ = self.0.lock().unwrap().send("the timer");
        }
    }

    // A reactor whose thread stops, here as a socket's callback panics,
    // aborts each socket still registered and wakes each timer at once,
    // rather than leave their waiters to their deadlines, and is handed out
    // no more. A poll that fails leaves the loop through the same guard.
    // @behavior NW-235
    #[test]
    fn a_reactor_that_stops_aborts_every_socket_and_timer_it_watches() {
        let reactor = Reactor::start().expect("a reactor starts");
        let (tx, rx) = mpsc::channel();
        let waiting: Arc<dyn Ready> = Arc::new(TellsAbort(Mutex::new(tx.clone())));
        let panics = Arc::new(PanicsOnceArmed(AtomicBool::new(false)));
        let failing: Arc<dyn Ready> = panics.clone();
        let addr = "127.0.0.1:0".parse().unwrap();
        let mut watched = mio::net::UdpSocket::bind(addr).expect("a UDP socket binds");
        let mut failed = mio::net::UdpSocket::bind(addr).expect("a UDP socket binds");
        let _watching = reactor
            .register(&mut watched, Arc::downgrade(&waiting))
            .expect("the socket registers");
        let _failing = reactor
            .register(&mut failed, Arc::downgrade(&failing))
            .expect("the socket registers");
        let _timer = reactor.wake_at(
            Instant::now() + Duration::from_secs(30),
            Waker::from(Arc::new(TellsWake(Mutex::new(tx)))),
        );
        panics.0.store(true, Ordering::Release);
        std::net::UdpSocket::bind(addr)
            .expect("a UDP socket binds")
            .send_to(b"x", failed.local_addr().unwrap())
            .expect("the datagram goes");
        let mut told: Vec<_> = (0..2)
            .filter_map(|_| rx.recv_timeout(Duration::from_secs(5)).ok())
            .collect();
        told.sort_unstable();
        assert_eq!(
            (told, reactor.stopped.load(Ordering::Acquire)),
            (vec!["the socket", "the timer"], true),
            "a reactor whose thread stops must abort each socket registered on it, wake each \
             timer at once, and mark itself stopped so no later socket registers on it"
        );
    }

    /// A socket that counts each turn it is given.
    #[derive(Default)]
    struct CountsTurns(AtomicUsize);

    impl Ready for CountsTurns {
        fn on_event(&self, _events: PollMask) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }

        fn abort(&self) {}
    }

    // A socket that spent its turn's bound waits for its next turn, and a
    // signal interrupting the reactor's wait in between must not drop it:
    // with no further event to come, it would wait until its deadline.
    // @behavior NW-174
    #[test]
    fn a_repoll_runs_once_past_an_interrupted_wait() {
        let poll = mio::Poll::new().expect("a poll opens");
        let reactor = Reactor::on(&poll).expect("a reactor is made");
        let counts = Arc::new(CountsTurns::default());
        let ready: Arc<dyn Ready> = counts.clone();
        reactor.repoll().push(Arc::downgrade(&ready));
        // Interrupted, then a round with no event, then a failure that stops
        // the thread.
        let mut waits = [
            Err(io::ErrorKind::Interrupted.into()),
            Ok(()),
            Err(io::ErrorKind::Other.into()),
        ]
        .into_iter();
        reactor.run(|_, _| waits.next().expect("the reactor stopped at the failure"));
        assert_eq!(
            counts.0.load(Ordering::SeqCst),
            1,
            "a socket asking for another turn must get it once in the round after a reactor \
             wait a signal interrupted"
        );
    }

    // A forked child shares its parent's epoll instance, where an inherited
    // socket's `<fd, file>` entry stands once for both, so a child letting the
    // socket go must leave the parent watching it.
    // @behavior NW-232
    #[test]
    fn a_registration_made_in_another_process_stays_watched_once_let_go() {
        let reactor = Reactor::current().expect("a reactor starts");
        let (tx, rx) = mpsc::channel();
        let ready: Arc<dyn Ready> = Arc::new(Tells(Mutex::new(tx)));
        let addr = "127.0.0.1:0".parse().unwrap();
        let mut socket = mio::net::UdpSocket::bind(addr).expect("a UDP socket binds");
        let mut registration = reactor
            .register(&mut socket, Arc::downgrade(&ready))
            .expect("the socket registers");
        while rx.recv_timeout(Duration::from_millis(50)).is_ok() {}
        registration.pid = Pid::parent();
        registration.deregister(&mut socket);
        let sender = std::net::UdpSocket::bind(addr).expect("a UDP socket binds");
        sender
            .send_to(b"x", socket.local_addr().unwrap())
            .expect("the datagram goes");
        assert!(
            rx.recv_timeout(Duration::from_secs(5)).is_ok(),
            "a forked child letting go of a guest's socket its parent watches must leave the \
             parent woken by that socket's events"
        );
    }

    // A waker asked to be woken at an instant is woken once it passes, with
    // nothing else waking it.
    #[test]
    fn a_timer_wakes_its_waker_once_due() {
        let reactor = Reactor::current().expect("a reactor starts");
        let (tx, rx) = mpsc::channel();
        let _timer = reactor.wake_at(
            Instant::now() + Duration::from_millis(50),
            Waker::from(Arc::new(TellsWake(Mutex::new(tx)))),
        );
        assert!(
            rx.recv_timeout(Duration::from_secs(5)).is_ok(),
            "a waker the reactor was asked to wake at an instant must be woken once it passes"
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

    // A request cancelled while its timer is armed leaves no timer behind, so
    // a guest dropping lookups never fills the reactor's timers.
    #[test]
    fn a_cancelled_request_leaves_no_timer() {
        let reactor = Reactor::start().expect("a reactor starts");
        let timing = reactor.clone();
        let later = Instant::now() + Duration::from_secs(30);
        let task = Task::new(
            async move {
                let mut timer = None;
                std::future::poll_fn(|cx| {
                    timer.get_or_insert_with(|| timing.wake_at(later, cx.waker().clone()));
                    Poll::<()>::Pending
                })
                .await;
            },
            Arc::default(),
        );
        task.poll();
        let held = reactor.timers.lock().unwrap().len();
        task.cancel();
        assert_eq!(
            (held, reactor.timers.lock().unwrap().len()),
            (1, 0),
            "a cancelled lookup's timer must leave the reactor with the lookup, not stay until due"
        );
    }
}
