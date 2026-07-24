//! Releasing Ruby's GVL for the guest-execution span so distinct Sandboxes on
//! distinct Threads run their guest code in parallel.
//!
//! `region` brackets one invocation's guest span, releasing the GVL when the
//! Sandbox asks for it; `reenter` re-acquires the GVL for a guest→host dispatch
//! callback, which is ordinary Ruby and must hold the lock. A thread-local flag
//! stops a nested dispatch from re-acquiring a second time, and both directions
//! run their closure through a `catch_unwind` trampoline because a panic must
//! never cross the C frame of `rb_thread_call_without_gvl` /
//! `rb_thread_call_with_gvl` (undefined behaviour). The flag is per-thread,
//! matching the one-thread-one-Sandbox contract, and is restored through a drop
//! guard so an unwinding panic cannot leave it corrupt for the next invocation.

use std::cell::Cell;
use std::os::raw::c_void;
use std::panic::{catch_unwind, resume_unwind, AssertUnwindSafe};
use std::ptr;

thread_local! {
    // True while this thread sits inside a region whose GVL we released and
    // have not yet re-acquired. Drives `reenter`'s one-shot re-acquire.
    static GVL_RELEASED: Cell<bool> = const { Cell::new(false) };
}

/// Run `f` (one invocation's guest-execution span) and return its value,
/// releasing the GVL for the span iff `release`. When holding, `f` runs inline.
pub(crate) fn region<F, R>(release: bool, f: F) -> R
where
    F: FnOnce() -> R,
{
    if !release {
        return f();
    }
    let _guard = ReleasedGuard::set(true);
    call_via(f, |func, data| unsafe {
        rb_sys::rb_thread_call_without_gvl(Some(func), data, None, ptr::null_mut())
    })
}

/// Run `f` (a guest→host dispatch callback) under the GVL. Re-acquires the GVL
/// when the enclosing region released it, and runs inline when the GVL is
/// already held — hold mode, or a nested dispatch whose outer frame re-acquired
/// it.
pub(crate) fn reenter<F, R>(f: F) -> R
where
    F: FnOnce() -> R,
{
    if !GVL_RELEASED.with(|g| g.get()) {
        return f();
    }
    let _guard = ReleasedGuard::set(false);
    call_via(f, |func, data| unsafe {
        rb_sys::rb_thread_call_with_gvl(Some(func), data)
    })
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

/// The `extern "C"` shim the `rb_thread_call_*` functions call: it runs the
/// erased closure under `catch_unwind` so a panic is captured here rather than
/// unwinding across the C frame, and stashes the outcome for `call_via`.
unsafe extern "C" fn trampoline<F, R>(data: *mut c_void) -> *mut c_void
where
    F: FnOnce() -> R,
{
    let ctx = unsafe { &mut *data.cast::<GvlCtx<F, R>>() };
    let f = ctx.f.take().expect("gvl trampoline runs its closure once");
    ctx.result = Some(catch_unwind(AssertUnwindSafe(f)));
    ptr::null_mut()
}

/// Drive `f` through one of the `rb_thread_call_*` entry points (`invoke`),
/// erasing it to the C callback shape and re-raising on the caller's thread any
/// panic the trampoline caught.
fn call_via<F, R>(
    f: F,
    invoke: impl FnOnce(unsafe extern "C" fn(*mut c_void) -> *mut c_void, *mut c_void) -> *mut c_void,
) -> R
where
    F: FnOnce() -> R,
{
    let mut ctx = GvlCtx::<F, R> {
        f: Some(f),
        result: None,
    };
    invoke(trampoline::<F, R>, ptr::addr_of_mut!(ctx).cast::<c_void>());
    match ctx.result.take().expect("gvl trampoline stores a result") {
        Ok(value) => value,
        Err(payload) => resume_unwind(payload),
    }
}
