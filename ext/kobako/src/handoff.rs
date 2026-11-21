//! The thread a run's guest stands on, apart from the Fiber that called it.
//!
//! The guest's Wasm frames stay on a thread of its own, so a callback runs in
//! its caller's own frame: a Fiber switch, an interrupt, or a raise there
//! crosses no Wasm frame, and two Fibers' runs on one Thread each keep their
//! own guest stack. Whatever the guest needs of Ruby comes back to the caller
//! as a job, run where the caller waits. The caller returns only once its
//! guest has ended, as it holds the Ruby objects the guest reaches; the guest
//! owns all else it reads, so one whose caller never comes back goes on
//! without it.
//!
//! The guest's threads are kept for the next run once one ends, as starting a
//! thread costs more than a run handing one over.

use std::any::Any;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::panic::{catch_unwind, resume_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::thread;

use kobako_runtime::fork::ProcessLocal;
use kobako_runtime::sched::{Current, Task};

/// The stack a guest's thread runs on: the Engine's Wasm stack and the host
/// frames of the calls beneath it.
const GUEST_STACK: usize = 8 << 20;

/// How often a side looks for what the other hands over before it sleeps, as a
/// callback is often back sooner than a sleep and a wakeup take.
const SPINS: u32 = 20_000;

pub(crate) type Panic = Box<dyn Any + Send>;

/// Work handed from one side to the other, which the hander waits on.
type Job = Box<dyn FnOnce() + Send>;

/// What the guest's thread hands its caller.
pub(crate) enum Message {
    /// Run this with the GVL held, then answer.
    Job(Job),
    /// The guest waits for a lock or an event: give the GVL up until it says
    /// it is done, then answer once it is held again.
    Wait(bool),
    /// What the guest ran for the caller ended, with the panic it ended by.
    Answer(Option<Panic>),
    /// The run ended, with the panic it ended by.
    Done(Option<Panic>),
}

/// What the caller hands the guest's thread while it waits.
enum Reply {
    /// What the guest asked for ended, with the panic it ended by.
    Answer(Option<Panic>),
    /// Run this on the guest's stack, as a guest block a callback runs, then
    /// answer.
    Job(Job),
}

/// What one thread hands another in order, which waits for the next. A side
/// may hand over twice before the other looks, as a guest that begins a wait
/// may ask for a signal to be handled before its caller saw the wait begin.
struct Mailbox<T> {
    values: Mutex<VecDeque<T>>,
    filled: AtomicBool,
    arrived: Condvar,
}

impl<T> Mailbox<T> {
    fn new() -> Self {
        Self {
            values: Mutex::new(VecDeque::new()),
            filled: AtomicBool::new(false),
            arrived: Condvar::new(),
        }
    }

    fn put(&self, value: T) {
        let mut values = self.values.lock().unwrap_or_else(PoisonError::into_inner);
        values.push_back(value);
        self.filled.store(true, Ordering::Release);
        self.arrived.notify_one();
    }

    fn take(&self) -> T {
        for _ in 0..SPINS {
            if self.filled.load(Ordering::Acquire) {
                break;
            }
            std::hint::spin_loop();
        }
        let mut values = self.values.lock().unwrap_or_else(PoisonError::into_inner);
        loop {
            if let Some(value) = values.pop_front() {
                self.filled.store(!values.is_empty(), Ordering::Release);
                return value;
            }
            values = self
                .arrived
                .wait(values)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }
}

/// Both directions between one run's caller and its guest's thread.
pub(crate) struct Line {
    to_caller: Mailbox<Message>,
    to_guest: Mailbox<Reply>,
    /// Whether the run keeps the GVL while its guest runs, so a wait hands it
    /// back to the caller to give up.
    keeps_gvl: bool,
    pending: Arc<AtomicBool>,
    /// Whether the caller is gone for good, as a Fiber left suspended in a
    /// callback and collected is.
    abandoned: AtomicBool,
    /// Ends the run's waits once its caller is gone.
    interrupt: Task,
}

impl Line {
    /// Have the guest go on without its caller: what it waits on is answered
    /// with nothing, and nothing more is handed over.
    pub(crate) fn abandon(&self) {
        self.abandoned.store(true, Ordering::SeqCst);
        self.interrupt.interrupt();
        self.to_guest.put(Reply::Answer(None));
    }

    fn is_abandoned(&self) -> bool {
        self.abandoned.load(Ordering::SeqCst)
    }

    /// What the guest's thread hands over next, waited for.
    pub(crate) fn take(&self) -> Message {
        self.to_caller.take()
    }

    /// Whether the run keeps the GVL while its guest runs.
    pub(crate) fn keeps_gvl(&self) -> bool {
        self.keeps_gvl
    }

    /// Run `job` as the guest asked, then answer it.
    pub(crate) fn answer(&self, job: Job) {
        let panic = catch_unwind(AssertUnwindSafe(job)).err();
        self.to_guest.put(Reply::Answer(panic));
    }

    /// Answer a wait once the GVL is held again.
    pub(crate) fn resume(&self) {
        self.to_guest.put(Reply::Answer(None));
    }

    /// Hand `f` to the guest's thread, which waits for an answer of its own,
    /// to run on the guest's stack, while `serve` serves the line until
    /// `Message::Answer` and hands back the panic it carried.
    pub(crate) fn hand_guest<R>(
        &self,
        f: impl FnOnce() -> R,
        serve: impl FnOnce() -> Option<Panic>,
    ) -> R {
        let mut result = None;
        let job: Box<dyn FnOnce() + Send + '_> =
            Box::new(AssertSend(|| result = Some(f())).into_fn());
        // SAFETY: only the lifetime is erased; `serve` returns only once the
        // guest ran the job or caught its panic, so nothing it borrows is gone
        // first.
        let job: Job = unsafe { std::mem::transmute(job) };
        self.to_guest.put(Reply::Job(job));
        if let Some(panic) = serve() {
            resume_unwind(panic);
        }
        result.expect("an answered job ran")
    }

    /// Hand `message` to the caller and wait for its answer, running on this
    /// stack what the caller hands over meanwhile.
    fn ask(&self, message: Message) {
        self.to_caller.put(message);
        loop {
            match self.to_guest.take() {
                Reply::Answer(panic) => {
                    if let Some(panic) = panic {
                        resume_unwind(panic);
                    }
                    return;
                }
                Reply::Job(job) => {
                    let panic = catch_unwind(AssertUnwindSafe(job)).err();
                    self.to_caller.put(Message::Answer(panic));
                }
            }
        }
    }
}

thread_local! {
    // The run a guest's thread carries out, while it does.
    static LINE: RefCell<Option<Arc<Line>>> = const { RefCell::new(None) };
}

fn line() -> Option<Arc<Line>> {
    LINE.with(|line| line.borrow().clone())
}

/// Whether the calling thread is a guest's, asking its caller for what needs
/// Ruby.
pub(crate) fn on_guest() -> bool {
    LINE.with(|line| line.borrow().is_some())
}

/// The run's pending flag, on a guest's thread.
pub(crate) fn pending_flag() -> Option<Arc<AtomicBool>> {
    line().map(|line| line.pending.clone())
}

/// Run `f` on the caller of the guest the calling thread carries, holding the
/// GVL, and answer what it does; `None` once the caller is gone, as nothing
/// of Ruby is left to run it.
pub(crate) fn on_caller<R>(f: impl FnOnce() -> R) -> Option<R> {
    let line = line().expect("only a guest's thread hands work to its caller");
    if line.is_abandoned() {
        return None;
    }
    let mut result = None;
    let job: Box<dyn FnOnce() + Send + '_> = Box::new(AssertSend(|| result = Some(f())).into_fn());
    // SAFETY: only the lifetime is erased; `ask` returns only once the caller
    // ran the job, caught its panic, or is gone and never runs it, so nothing
    // it borrows is gone first.
    let job: Job = unsafe { std::mem::transmute(job) };
    line.ask(Message::Job(job));
    result
}

/// Run `wait` with the GVL given up for it, when the run keeps the GVL; the
/// caller gives it up and takes it back around the wait.
pub(crate) fn wait_releasing(wait: &mut dyn FnMut()) {
    let line = line().expect("only a guest's thread waits through its caller");
    if !line.keeps_gvl || line.is_abandoned() {
        return wait();
    }
    line.to_caller.put(Message::Wait(true));
    wait();
    line.ask(Message::Wait(false));
}

/// A closure whose captures the other side touches only while this side
/// waits for it, so it crosses threads though they are not `Send`.
struct AssertSend<F>(F);

// SAFETY: see the type; the side that made it waits until it ran.
unsafe impl<F> Send for AssertSend<F> {}

impl<F: FnOnce()> AssertSend<F> {
    fn into_fn(self) -> impl FnOnce() + Send {
        move || {
            let this = self;
            (this.0)()
        }
    }
}

/// A guest's thread waiting for the next run.
struct Worker {
    runs: Arc<Mailbox<Job>>,
}

/// The guest threads waiting for a run, this process's own.
static IDLE: ProcessLocal<Vec<Worker>> = ProcessLocal::new();

/// Start `run` on a guest's thread carrying `task`, and hand back the line its
/// caller serves until `Message::Done`. The run owns all it reads, as its
/// caller may never come back for it.
pub(crate) fn start(
    task: Current,
    keeps_gvl: bool,
    pending: Arc<AtomicBool>,
    run: impl FnOnce() + Send + 'static,
) -> Arc<Line> {
    let line = Arc::new(Line {
        to_caller: Mailbox::new(),
        to_guest: Mailbox::new(),
        keeps_gvl,
        pending,
        abandoned: AtomicBool::new(false),
        interrupt: task.task(),
    });
    let guest_line = line.clone();
    let job: Job = Box::new(move || {
        LINE.with(|held| *held.borrow_mut() = Some(guest_line.clone()));
        let panic = catch_unwind(AssertUnwindSafe(|| task.carry(run))).err();
        LINE.with(|held| *held.borrow_mut() = None);
        guest_line.to_caller.put(Message::Done(panic));
    });
    let idle = IDLE.with(|idle| idle.get_or_insert_with(Vec::new).pop());
    match idle {
        Some(worker) => worker.runs.put(job),
        None => spawn(job),
    }
    line
}

fn spawn(first: Job) {
    let runs = Arc::new(Mailbox::new());
    runs.put(first);
    thread::Builder::new()
        .name("kobako-guest".into())
        .stack_size(GUEST_STACK)
        .spawn(move || loop {
            let run = runs.take();
            run();
            IDLE.with(|idle| {
                idle.get_or_insert_with(Vec::new)
                    .push(Worker { runs: runs.clone() })
            });
        })
        .expect("a guest's thread starts");
}
