//! Releasing Ruby's GVL for an invocation's guest span so distinct Sandboxes on
//! distinct Threads run their guest code in parallel.
//!
//! `region` brackets one invocation's guest span, releasing the GVL when the
//! Sandbox asks for it; `reenter` re-acquires the GVL for a guest→host dispatch
//! callback, which is ordinary Ruby and must hold the lock. A thread-local flag
//! stops a nested dispatch from re-acquiring a second time, and both directions
//! run their closure through a `catch_unwind` trampoline because a panic must
//! never cross the C frame of `rb_thread_call_without_gvl2` /
//! `rb_thread_call_with_gvl` (undefined behaviour). The flag is per-thread,
//! matching the one-thread-one-Sandbox contract, and is restored through a drop
//! guard so an unwinding panic cannot leave it corrupt for the next invocation.
//!
//! Neither direction raises where it returns, since that jumps over every Rust
//! frame of the run and what they hold: a lock's guard, this flag. A release
//! uses `rb_thread_call_without_gvl2`, which declines rather than raises, and
//! a run releasing the GVL defers Ruby's interrupts (`masked`) outside its
//! callbacks, as `rb_thread_call_with_gvl` checks for one as it gives the GVL
//! back. An interrupt waits for the run to end instead.

use std::cell::Cell;
use std::os::raw::c_void;
use std::panic::{catch_unwind, resume_unwind, AssertUnwindSafe};
use std::ptr;

use kobako_runtime::sched::{run_interruptible, Blocking, Task};

use magnus::block::Proc;
use magnus::{prelude::*, Error as MagnusError, Ruby, Value};

use super::escape;

thread_local! {
    // True while this thread sits inside a region whose GVL we released and
    // have not yet re-acquired. Drives `reenter`'s one-shot re-acquire.
    static GVL_RELEASED: Cell<bool> = const { Cell::new(false) };
}

pub(crate) fn region<F, R>(release: bool, f: F) -> R
where
    F: FnOnce() -> R,
{
    if !release {
        return f();
    }
    // An interrupt the thread receives meanwhile stops each wait the run meets,
    // for a lock or a host-side event; the interrupt itself still waits for the
    // run to end. One landing before or after the run itself, while the guest
    // is set up or torn down, still belongs to this region and goes with it.
    let task = Task::current();
    let task_ptr = ptr::addr_of!(task).cast_mut().cast::<c_void>();
    run_interruptible(|| {
        let mut f = f;
        loop {
            let released = ReleasedGuard::set(true);
            let ran = call_via(f, |func, data| {
                // An interrupt that came before the region, which Ruby declines
                // to start under or the run's mask defers, stops the waits as one
                // inside it does, as a signal pending when a Linux task sleeps
                // ends the sleep.
                if escape::pending() || interrupt_pending() {
                    task.interrupt();
                }
                // SAFETY: `func` and `data` come from `call_via`, which keeps the
                // closure alive across the call; `task` outlives it on this frame,
                // and Ruby calls `interrupt_task` with it only while the region runs.
                unsafe {
                    rb_sys::rb_thread_call_without_gvl2(
                        Some(func),
                        data,
                        Some(interrupt_task),
                        task_ptr,
                    )
                }
            });
            drop(released);
            match ran {
                Ok(value) => return value,
                // A run holds the interrupt and raises it once back in Ruby.
                Err(declined) if escape::in_run() => {
                    escape::hold_interrupts();
                    f = declined;
                }
                // Outside a run nothing would raise a held interrupt, so it
                // stays with Ruby, which raises it as this call returns; the
                // wait runs with the GVL kept and ends at once, interrupted.
                Err(declined) => {
                    task.interrupt();
                    return declined();
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

/// Have the thread's run stop waiting for locks, called by Ruby without the GVL
/// as the region's unblocking function.
unsafe extern "C" fn interrupt_task(task: *mut c_void) {
    // SAFETY: `region` passes its own `Task`, alive for the whole region
    // Ruby calls this from.
    let task = unsafe { &*task.cast::<Task>() };
    task.interrupt();
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
        region(holds(), wait);
    }
}

/// Runs inline when the GVL is already held — hold mode, or a nested
/// dispatch whose outer frame re-acquired it.
pub(crate) fn reenter<F, R>(f: F) -> R
where
    F: FnOnce() -> R,
{
    if holds() {
        return f();
    }
    let _guard = ReleasedGuard::set(false);
    // Ruby checks for interrupts as it gives the GVL back on the way out, and
    // raising there would jump over every Rust frame of the run. The run masks
    // them (`masked`), so the callback alone takes them as they come.
    let reentered = |f: F| {
        let ruby = Ruby::get().expect("a reentered thread holds the GVL");
        let (value, raised) = handle_interrupt(&ruby, "immediate", f);
        if let Err(error) = raised {
            escape::keep(error);
        }
        value.expect("a callback run under handle_interrupt returns")
    };
    // SAFETY: `func` and `data` come from `call_via`, which keeps the
    // closure alive across the call, and this thread is a Ruby thread
    // inside a region that released the GVL, as the call requires.
    call_via(
        || reentered(f),
        |func, data| unsafe { rb_sys::rb_thread_call_with_gvl(Some(func), data) },
    )
    .ok()
    .expect("rb_thread_call_with_gvl always runs its function")
}

/// Run `f` as a run that gives up the GVL when `release` says so, with Ruby's
/// interrupts deferred until it is back in Ruby, where one comes as the error.
/// A run keeping the GVL meets no interrupt check outside its callbacks.
pub(crate) fn masked<R>(
    ruby: &Ruby,
    release: bool,
    f: impl FnOnce() -> R,
) -> Result<R, MagnusError> {
    if !release {
        return Ok(f());
    }
    let (value, raised) = handle_interrupt(ruby, "never", f);
    raised?;
    Ok(value.expect("a run under handle_interrupt returns"))
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
    /// Puts back the body of the call around this one.
    struct Restore(Option<*mut (dyn FnMut() + 'static)>);

    impl Drop for Restore {
        fn drop(&mut self) {
            BODY.with(|body| body.set(self.0));
        }
    }

    fn yielded(ruby: &Ruby, _args: &[Value], _block: Option<Proc>) -> Value {
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
    // SAFETY: only the lifetime is erased; `Restore` takes the pointer back
    // before `body` goes out of scope, and nothing else keeps it.
    let body: *mut (dyn FnMut() + 'static) = unsafe { std::mem::transmute(body) };
    let _restore = Restore(BODY.with(|current| current.replace(Some(body))));
    let mask = ruby.hash_new();
    let raised = mask
        .aset(ruby.class_object(), ruby.to_symbol(timing))
        .and_then(|()| {
            ruby.class_thread()
                .funcall_with_block::<_, _, Value>(
                    "handle_interrupt",
                    (mask,),
                    ruby.proc_new(yielded),
                )
                .map(|_| ())
        });
    drop(_restore);
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
