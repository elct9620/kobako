//! A run of a Sandbox, its guest on a thread of its own while the calling Fiber
//! serves it, giving up the GVL while the guest runs when the Sandbox asks.
//!
//! `run` hands the guest to its thread and serves what it asks of Ruby in this
//! frame, holding the GVL; `reenter` is how the guest's thread asks, and runs
//! inline where the GVL is already held. Ruby runs only where no guest frame
//! stands, so a raise, a trap handler, or a Fiber switch in a callback crosses
//! none, and a callback takes interrupts as the Host App's own mask allows.
//! What Ruby delivers while the guest runs without the GVL is held by the run
//! before the next callback starts, and the run raises it once it ends.
//!
//! A wait without the GVL uses `rb_thread_call_without_gvl2`, which declines
//! rather than raises. A thread-local flag says a thread is inside such a wait,
//! so a signal handled there takes the GVL back once; a Ruby thread may go on
//! on another native thread once it takes the GVL back, so the flag is set only
//! inside the wait. Each direction runs its closure through a `catch_unwind`
//! trampoline, as a panic must never cross a C frame.

use std::cell::Cell;
use std::ffi::CString;
use std::os::raw::{c_char, c_int, c_long, c_void};
use std::panic::{catch_unwind, resume_unwind, AssertUnwindSafe};
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::Duration;

use kobako_runtime::sched::{run_interruptible, Blocking, Current, Task};

use magnus::block::Proc;
use magnus::gc::Marker;
use magnus::rb_sys::FromRawValue;
use magnus::typed_data::DataTypeFunctions;
use magnus::TypedData;
use magnus::{prelude::*, Error as MagnusError, RModule, Ruby, Value};

use super::escape;
use super::fork;
use super::handoff::{self, Message};

thread_local! {
    // True while this thread sits inside a region whose GVL we released and
    // have not yet re-acquired. Drives `reenter`'s one-shot re-acquire.
    static GVL_RELEASED: Cell<bool> = const { Cell::new(false) };
}

fn region<F, R>(release: bool, f: F) -> R
where
    F: FnOnce() -> R,
{
    if !release {
        return f();
    }
    // What Ruby wakes the thread for meanwhile signals each wait the run meets,
    // for a lock or a host-side event, which hands it to `GvlFreeWait`; what it
    // raises waits for the run to end. One landing before or after the run
    // itself, while the guest is set up or torn down, still belongs to this
    // region and goes with it.
    let task = Task::current();
    let task_ptr = ptr::addr_of!(task).cast_mut().cast::<c_void>();
    // A Ruby thread may go on on another native thread once it takes the GVL
    // back, so what this thread-local state says holds only while `f` runs,
    // on the thread running it.
    let current = Current::get();
    let f = Cell::new(Some(f));
    let take = || f.take().expect("a region runs its closure once");
    let mut released = || {
        let _released = ReleasedGuard::set(true);
        current.carry(take())
    };
    run_interruptible(|| {
        loop {
            let ran = call_via(&mut released, |func, data| {
                // What came before the region, which Ruby declines to start
                // under or the run's mask defers, reaches the waits as one
                // inside it does, as a signal pending when a Linux task sleeps
                // wakes the sleep.
                if escape::pending() {
                    task.interrupt();
                } else if interrupt_pending() {
                    task.signal();
                }
                // SAFETY: `func` and `data` come from `call_via`, which keeps the
                // closure alive across the call; `task` outlives it on this frame,
                // and Ruby calls `signal_task` with it only while the region runs.
                unsafe {
                    rb_sys::rb_thread_call_without_gvl2(
                        Some(func),
                        data,
                        Some(signal_task),
                        task_ptr,
                    )
                }
            });
            match ran {
                Ok(value) => return value,
                // A run holds the interrupt and raises it once back in Ruby.
                // One its mask defers stays queued, so the waits are signalled
                // to take it as a callback would.
                Err(_) if escape::is_in_run() => {
                    keeping_held_jump(escape::hold_interrupts);
                    task.signal();
                }
                // Outside a run nothing would raise a held interrupt, so it
                // stays with Ruby, which raises it as this call returns; the
                // wait runs with the GVL kept and ends at once, interrupted.
                Err(_) => {
                    task.interrupt();
                    return current.carry(take());
                }
            }
        }
    })
}

/// Whether Ruby holds an interrupt or a signal for this thread, deferred or
/// not, asked without running it.
fn interrupt_pending() -> bool {
    // SAFETY: `rb_thread_interrupted` only reads the current thread's flags,
    // which Ruby allows even without the GVL.
    unsafe { rb_sys::rb_thread_interrupted(rb_sys::rb_thread_current()) != 0 }
}

/// Signal the thread's run, called by Ruby without the GVL as the region's
/// unblocking function, whenever it wakes the thread: for an exception, a
/// trap, or `Thread#wakeup`, which only the GVL tells apart.
unsafe extern "C" fn signal_task(task: *mut c_void) {
    // SAFETY: `region` passes its own `Task`, alive for the whole region
    // Ruby calls this from.
    let task = unsafe { &*task.cast::<Task>() };
    task.signal();
}

/// Whether this thread holds the GVL: outside any released region, or
/// inside a dispatch that re-acquired it.
fn holds() -> bool {
    !GVL_RELEASED.with(|g| g.get())
}

/// A thread waits for a lock or a host-side event without the GVL, so a
/// holder asking a Filesystem written in Ruby, or a mount's block, can take
/// the GVL and finish, and other Threads run while the guest sleeps.
pub(crate) struct GvlFreeWait;

impl Blocking for GvlFreeWait {
    fn wait(&self, wait: &mut dyn FnMut()) {
        if handoff::on_guest() {
            return handoff::wait_releasing(wait);
        }
        if !(holds() && escape::is_in_run()) {
            return region(holds(), wait);
        }
        // A run keeping the GVL defers interrupts across its wait, so taking
        // the GVL back to handle a signal never raises where it gives the GVL
        // up again.
        let ruby = Ruby::get().expect("a Thread keeping the GVL holds it");
        let (_, raised) = handle_interrupt(&ruby, "never", || region(true, wait));
        if let Err(error) = raised {
            escape::keep(error);
        }
    }

    /// Take what Ruby woke the thread for, as a callback takes it; one that
    /// raises ends the run's waits, and the run raises it once it ends.
    /// Outside a run nothing would raise it, so it stays with Ruby and ends the
    /// wait at once.
    fn handle_signal(&self) {
        if handoff::on_guest() {
            handoff::on_caller(|| self.handle_signal());
            return;
        }
        if escape::is_in_run() {
            reenter(escape::hold_interrupts);
        }
        if !escape::is_in_run() || escape::pending() {
            Task::current().interrupt();
        }
    }

    /// The run of the calling Fiber, asked only while the GVL is held, as a
    /// Fiber may resume on another native thread once it gave the GVL up.
    fn current(&self) -> Option<Current> {
        if !holds_gvl() {
            return None;
        }
        escape::current_run().map(|run| run.task().clone())
    }
}

/// Whether the calling thread holds the GVL: a Ruby thread outside any region
/// of this module, where alone a Ruby thread gives it up.
pub(crate) fn holds_gvl() -> bool {
    // SAFETY: `ruby_native_thread_p` only asks whether the calling thread is
    // Ruby's, on any thread.
    !handoff::on_guest() && unsafe { rb_sys::ruby_native_thread_p() } != 0 && holds()
}

/// Run `f`, which calls Ruby, holding the GVL: on the run's caller when a
/// guest's thread asks, inline when the GVL is already held. `None` when the
/// run's caller is gone, as a Fiber left suspended in a callback and collected
/// is, so nothing of Ruby is reached.
pub(crate) fn reenter<F, R>(f: F) -> Option<R>
where
    F: FnOnce() -> R,
{
    if handoff::on_guest() {
        return handoff::on_caller(|| reenter(f)).flatten();
    }
    if holds() {
        return Some(keeping_held_jump(f));
    }
    let _guard = ReleasedGuard::set(false);
    // Ruby checks for interrupts as it gives the GVL back on the way out, and
    // raising there would jump over every Rust frame of the run. A wait inside
    // a run masks them (`GvlFreeWait`), so the callback alone takes them as
    // they come.
    let run_callback = |f: F| {
        let ruby = Ruby::get().expect("a thread back in Ruby holds the GVL");
        let (value, raised) = keeping_held_jump(|| handle_interrupt(&ruby, "immediate", f));
        if let Err(error) = raised {
            escape::keep(error);
        }
        value.expect("a callback run under handle_interrupt returns")
    };
    // SAFETY: `func` and `data` come from `call_via`, which keeps the
    // closure alive across the call, and this thread is a Ruby thread
    // inside a region that released the GVL, as the call requires.
    let ran = call_via(
        || run_callback(f),
        |func, data| unsafe { rb_sys::rb_thread_call_with_gvl(Some(func), data) },
    );
    Some(
        ran.ok()
            .expect("rb_thread_call_with_gvl always runs its function"),
    )
}

/// Run `f`, which calls Ruby, so a `throw` the run holds keeps its target.
/// CRuby keeps where a `throw` goes on the Fiber rather than in anything
/// `escape` can hold, and a `catch` completing inside `f` clears it;
/// `rb_ensure` puts it back after running its ensure function, which is `f`.
fn keeping_held_jump<F, R>(f: F) -> R
where
    F: FnOnce() -> R,
{
    /// The body `rb_ensure` runs before `f`, which does nothing.
    unsafe extern "C" fn nothing(_: rb_sys::VALUE) -> rb_sys::VALUE {
        rb_sys::Qnil as rb_sys::VALUE
    }

    /// `trampoline`, taking its `GvlCtx` as the `VALUE` `rb_ensure` passes.
    unsafe extern "C" fn ensure<F, R>(data: rb_sys::VALUE) -> rb_sys::VALUE
    where
        F: FnOnce() -> R,
    {
        // SAFETY: `data` is the `GvlCtx<F, R>` `call_via` handed over.
        unsafe { trampoline::<F, R>(data as *mut c_void) as rb_sys::VALUE }
    }

    if !escape::pending() {
        return f();
    }
    // SAFETY: `data` comes from `call_via`, which keeps the closure alive
    // across the call; `ensure` runs it once, under `catch_unwind`.
    call_via(f, |_, data| unsafe {
        rb_sys::rb_ensure(
            Some(nothing),
            rb_sys::Qnil as rb_sys::VALUE,
            Some(ensure::<F, R>),
            data as rb_sys::VALUE,
        );
        ptr::null_mut()
    })
    .ok()
    .expect("rb_ensure always runs its ensure function")
}

/// Run `f` as one run of a Sandbox on a guest's thread, serving what it asks of
/// Ruby here, giving up the GVL while it runs when `release` says so, and
/// raising what the run kept once it ends.
///
/// `setup` builds what the guest runs inside the run, so each Ruby object it
/// pins is kept for the run.
pub(crate) fn run<R, F>(
    ruby: &Ruby,
    release: bool,
    setup: impl FnOnce() -> Result<F, MagnusError>,
) -> Result<R, MagnusError>
where
    R: Send + 'static,
    F: FnOnce() -> R + Send + 'static,
{
    let run = escape::Run::enter(ruby)?;
    let f = setup()?;
    let ran = serve(ruby, release, run.state(), f);
    fork::tell_pitchfork_past_trust(ruby);
    run.finish()?;
    Ok(ran)
}

/// Run `f` on a guest's thread carrying the run's task and serve it until it
/// ends. Each job it hands over runs here holding the GVL, in this frame, so
/// what a callback raises or switches to never crosses the guest's frames.
fn serve<R: Send + 'static>(
    ruby: &Ruby,
    release: bool,
    state: &Arc<escape::RunState>,
    f: impl FnOnce() -> R + Send + 'static,
) -> R {
    let ran = Arc::new(Mutex::new(None));
    let answered = ran.clone();
    let line = handoff::start(
        state.task().clone(),
        !release,
        state.pending_flag(),
        move || {
            let value = f();
            *answered.lock().unwrap_or_else(PoisonError::into_inner) = Some(value);
        },
    );
    let caller = ruby.obj_wrap(RunCaller {
        line: line.clone(),
        fiber: escape::fiber_id(),
        run: state.clone(),
        served: AtomicBool::new(false),
    });
    let ended = state.lending(&line, || run_interruptible(|| serve_line(&line)));
    caller.served.store(true, Ordering::SeqCst);
    std::hint::black_box(caller);
    if let Some(panic) = ended {
        resume_unwind(panic);
    }
    let value = ran.lock().unwrap_or_else(PoisonError::into_inner).take();
    value.expect("a run that ended without a panic answered")
}

/// A run's caller as Ruby's collector sees it. It stands on the caller's stack
/// while the caller serves the run, so the collector frees it with a Fiber left
/// suspended in a callback and never resumed, and the guest then goes on
/// without its caller, every wait of it interrupted. It marks the Ruby objects
/// the run pinned, so they go with it.
#[derive(TypedData)]
#[magnus(class = "Kobako::RunCaller", free_immediately, mark, size)]
struct RunCaller {
    line: Arc<handoff::Line>,
    fiber: escape::FiberId,
    run: Arc<escape::RunState>,
    served: AtomicBool,
}

impl DataTypeFunctions for RunCaller {
    fn mark(&self, marker: &Marker) {
        for pinned in self.run.pinned() {
            // SAFETY: each is a live object the run pinned, unpinned only once
            // its holder lets it go.
            marker.mark(unsafe { Value::from_raw(pinned) });
        }
    }
}

impl Drop for RunCaller {
    fn drop(&mut self) {
        if !self.served.load(Ordering::SeqCst) {
            self.line.abandon();
            escape::forget(self.fiber);
        }
    }
}

/// Defines the class the `#[magnus::wrap]` name resolves to, and looks it up
/// once, before any object is wrapped. The quantum is read here, as close to
/// where CRuby reads its own as an extension stands.
pub(crate) fn register(kobako: RModule) -> Result<(), MagnusError> {
    let ruby = Ruby::get().expect("Ruby thread");
    kobako.define_class("RunCaller", ruby.class_object())?;
    RunCaller::class(&ruby);
    quantum();
    Ok(())
}

/// Run `f` on the guest's own stack, as a guest block a callback calls
/// re-enters the guest where its frames stand, serving the run meanwhile.
pub(crate) fn on_guest_stack<R>(line: &handoff::Line, f: impl FnOnce() -> R) -> R {
    line.hand_guest(f, || serve_line(line))
}

/// Serve what the guest hands over until the run ends or what was handed to it
/// is answered, with the panic it ended by.
fn serve_line(line: &handoff::Line) -> Option<handoff::Panic> {
    let task = Task::current();
    let release = !line.keeps_gvl();
    let mut gives_up = release;
    loop {
        let message = if gives_up {
            take_released(line, &task)
        } else {
            take_in_turn(line, &task)
        };
        match message {
            Message::Job(job) => {
                // What Ruby delivered while the guest ran is the run's, held
                // before the callback starts, so it never lands in the
                // callback as if the callback raised it.
                keeping_held_jump(escape::hold_interrupts);
                line.answer(job);
            }
            Message::Wait(true) => gives_up = true,
            Message::Wait(false) => {
                gives_up = release;
                line.begin_turn();
                line.resume();
            }
            Message::Answer(panic) | Message::Done(panic) => return panic,
        }
    }
}

/// What the guest hands over next, waited for keeping the GVL for what is left
/// of the caller's quantum, then giving it up to whichever Thread waits, as
/// CRuby's timer has a running Thread do. The mailbox is looked at once
/// before the time is judged, and nothing handed over starts the quantum
/// again, so a guest that never comes back, or one that never stops asking,
/// still lets the other Threads run.
fn take_in_turn(line: &handoff::Line, task: &Task) -> Message {
    loop {
        if let Some(message) = line.take_within(line.turn_left(quantum())) {
            return message;
        }
        yield_turn(task);
        line.begin_turn();
    }
}

/// Hand the GVL to a Thread waiting for it, then take what Ruby delivered, as
/// `rb_thread_schedule` does, which gives nothing up when no Thread waits. What
/// that raises is the run's, held, and ends the guest as an interrupt does.
fn yield_turn(task: &Task) {
    let scheduled = keeping_held_jump(|| {
        magnus::rb_sys::protect(|| {
            // SAFETY: the caller holds the GVL, which `rb_thread_schedule`
            // gives up and takes back; it raises only into this `protect`.
            unsafe { rb_sys::rb_thread_schedule() };
            rb_sys::Qnil as rb_sys::VALUE
        })
    });
    if let Err(raised) = scheduled {
        escape::keep(raised);
    }
    if escape::pending() {
        task.interrupt();
    }
}

/// How long a run's caller keeps the GVL while it waits for its guest: the
/// quantum CRuby gives each Thread, which Ruby 3.4 and later read from
/// `RUBY_THREAD_TIMESLICE` as the process starts.
fn quantum() -> Duration {
    static QUANTUM: OnceLock<Duration> = OnceLock::new();
    *QUANTUM.get_or_init(|| {
        let reads_timeslice = (
            rb_sys::RUBY_API_VERSION_MAJOR,
            rb_sys::RUBY_API_VERSION_MINOR,
        ) >= (3, 4);
        let set = reads_timeslice
            .then(|| std::env::var("RUBY_THREAD_TIMESLICE").ok())
            .flatten()
            .and_then(|value| timeslice_ms(&value));
        Duration::from_millis(set.unwrap_or(DEFAULT_QUANTUM_MS).into())
    })
}

/// CRuby's `thread_default_quantum_ms`.
const DEFAULT_QUANTUM_MS: u32 = 100;

/// The milliseconds `value` names as CRuby reads `RUBY_THREAD_TIMESLICE`:
/// `strtol` in any base, taken only above zero and within 32 bits.
fn timeslice_ms(value: &str) -> Option<u32> {
    extern "C" {
        fn strtol(text: *const c_char, end: *mut *mut c_char, base: c_int) -> c_long;
    }
    let text = CString::new(value).ok()?;
    // SAFETY: `text` is a NUL-terminated string alive across the call, and a
    // null `end` asks `strtol` not to say where it stopped.
    let quantum = unsafe { strtol(text.as_ptr(), ptr::null_mut(), 0) };
    u32::try_from(quantum).ok().filter(|&ms| ms > 0)
}

/// `rb_nogvl` declines to run its function while an interrupt is pending,
/// rather than raising.
const RB_NOGVL_INTR_FAIL: c_int = 0x1;

/// `rb_nogvl` may hand its function to the Fiber scheduler's
/// `blocking_operation_wait`, which runs it on another thread while the Fiber
/// waits; a Ruby without the hook ignores it.
const RB_NOGVL_OFFLOAD_SAFE: c_int = 0x4;

/// What a wait without the GVL hands Ruby's unblocking function.
struct Unblock<'a> {
    task: &'a Task,
    line: &'a handoff::Line,
}

/// Signal the run and wake the caller's wait, called by Ruby without the GVL
/// whenever it wakes the thread, or as a Fiber scheduler cancels the wait.
unsafe extern "C" fn unblock_wait(data: *mut c_void) {
    // SAFETY: `released_wait` passes its own `Unblock`, alive for the whole
    // wait Ruby calls this from.
    let unblock = unsafe { &*data.cast::<Unblock<'_>>() };
    unblock.task.signal();
    unblock.line.wake_caller();
}

/// What the guest hands over next, waited for without the GVL: within a spin
/// first, as a callback mostly comes back that soon, then in a wait a Fiber
/// scheduler may hand to another thread, so the Thread's other Fibers run while
/// the guest does.
fn take_released(line: &handoff::Line, task: &Task) -> Message {
    let unblock = Unblock { task, line };
    loop {
        if let Some(message) = released_wait(&unblock, 0, || line.take_spinning()) {
            return message;
        }
        let waited = released_wait(&unblock, RB_NOGVL_OFFLOAD_SAFE, || line.take_until_woken());
        if let Some(message) = waited {
            return message;
        }
    }
}

/// Run `take` once without the GVL. What Ruby woke the thread for, or declined
/// to wait under, signals each wait the run meets, as a signal pending when a
/// Linux task sleeps wakes the sleep; what a Fiber scheduler raises where it
/// takes the wait over is the run's, held and ending its waits.
fn released_wait(
    unblock: &Unblock<'_>,
    flags: c_int,
    mut take: impl FnMut() -> Option<Message>,
) -> Option<Message> {
    let data = ptr::addr_of!(*unblock).cast_mut().cast::<c_void>();
    // A Ruby thread may go on on another native thread once it takes the GVL
    // back, so the thread-local flag holds only while the wait runs.
    let mut released = || {
        let _released = ReleasedGuard::set(true);
        take()
    };
    let mut taken = None;
    let protected = magnus::rb_sys::protect(|| {
        if escape::pending() {
            unblock.task.interrupt();
        } else if interrupt_pending() {
            unblock.task.signal();
        }
        // SAFETY: `func` and `data` come from `call_via`, which keeps the
        // closure alive across the call; `unblock` outlives it, and Ruby calls
        // `unblock_wait` with it only while the wait runs.
        taken = Some(call_via(&mut released, |func, closure| unsafe {
            rb_sys::rb_nogvl(
                Some(func),
                closure,
                Some(unblock_wait),
                data,
                RB_NOGVL_INTR_FAIL | flags,
            )
        }));
        rb_sys::Qnil as rb_sys::VALUE
    });
    match (protected, taken) {
        (Ok(_), Some(Ok(message))) => message,
        // The run holds the interrupt and raises it once it ends, and the
        // waits are signalled to take it as a callback would.
        (Ok(_), _) => {
            keeping_held_jump(escape::hold_interrupts);
            unblock.task.signal();
            None
        }
        (Err(raised), _) => {
            escape::keep(raised);
            unblock.task.interrupt();
            None
        }
    }
}

thread_local! {
    // The body the block `handle_interrupt` yields to runs, kept here as a
    // block takes no closure.
    static BODY: Cell<Option<*mut (dyn FnMut() + 'static)>> = const { Cell::new(None) };
}

/// `Thread.handle_interrupt(Object => timing) { f }`: what `f` returned, if it
/// ran to its end, and what the call raised, as one deferred interrupt comes
/// as the mask is lifted.
fn handle_interrupt<R>(
    ruby: &Ruby,
    timing: &str,
    f: impl FnOnce() -> R,
) -> (Option<R>, Result<(), MagnusError>) {
    /// The body of the call around this one, put back on drop.
    struct OuterBody(Option<*mut (dyn FnMut() + 'static)>);

    impl Drop for OuterBody {
        fn drop(&mut self) {
            BODY.with(|body| body.set(self.0));
        }
    }

    fn run_body(ruby: &Ruby, _args: &[Value], _block: Option<Proc>) -> Value {
        if let Some(body) = BODY.with(Cell::get) {
            // SAFETY: `handle_interrupt` set it for the length of this call,
            // which yields to this block at most once, on this thread.
            unsafe { (*body)() };
        }
        ruby.qnil().as_value()
    }

    let mut f = Some(f);
    let mut value = None;
    let mut body = || value = f.take().map(|f| f());
    let body: &mut dyn FnMut() = &mut body;
    // SAFETY: only the lifetime is erased; `OuterBody` takes the pointer back
    // before `body` goes out of scope, and nothing else keeps it.
    let body: *mut (dyn FnMut() + 'static) = unsafe { std::mem::transmute(body) };
    let outer = OuterBody(BODY.with(|current| current.replace(Some(body))));
    let mask = ruby.hash_new();
    let raised = mask
        .aset(ruby.class_object(), ruby.to_symbol(timing))
        .and_then(|()| {
            ruby.class_thread()
                .funcall_with_block::<_, _, Value>(
                    "handle_interrupt",
                    (mask,),
                    ruby.proc_new(run_body),
                )
                .map(|_| ())
        });
    drop(outer);
    (value, raised)
}

/// Restores `GVL_RELEASED` to its prior value on drop, so an unwinding panic
/// through a released region cannot leave the flag stuck.
struct ReleasedGuard {
    prev: bool,
}

impl ReleasedGuard {
    fn set(value: bool) -> Self {
        Self {
            prev: GVL_RELEASED.with(|g| g.replace(value)),
        }
    }
}

impl Drop for ReleasedGuard {
    fn drop(&mut self) {
        GVL_RELEASED.with(|g| g.set(self.prev));
    }
}

struct GvlCtx<F, R> {
    f: Option<F>,
    result: Option<std::thread::Result<R>>,
}

/// Runs the closure under `catch_unwind`, so a panic is captured here rather
/// than unwinding across the C frame.
unsafe extern "C" fn trampoline<F, R>(data: *mut c_void) -> *mut c_void
where
    F: FnOnce() -> R,
{
    // SAFETY: `call_via` passes its own `GvlCtx<F, R>`, borrowed by nothing
    // else for the length of the call.
    let ctx = unsafe { &mut *data.cast::<GvlCtx<F, R>>() };
    let f = ctx.f.take().expect("gvl trampoline runs its closure once");
    ctx.result = Some(catch_unwind(AssertUnwindSafe(f)));
    ptr::null_mut()
}

/// Run `f` through `invoke`, or hand it back when Ruby ran nothing because an
/// interrupt is pending, as `rb_thread_call_without_gvl2` does.
fn call_via<F, R>(
    f: F,
    mut invoke: impl FnMut(unsafe extern "C" fn(*mut c_void) -> *mut c_void, *mut c_void) -> *mut c_void,
) -> Result<R, F>
where
    F: FnOnce() -> R,
{
    let mut ctx = GvlCtx::<F, R> {
        f: Some(f),
        result: None,
    };
    invoke(trampoline::<F, R>, ptr::addr_of_mut!(ctx).cast::<c_void>());
    match ctx.result.take() {
        Some(Ok(value)) => Ok(value),
        Some(Err(payload)) => resume_unwind(payload),
        None => Err(ctx.f.take().expect("a declined call keeps its closure")),
    }
}
