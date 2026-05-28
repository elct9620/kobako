//! `mrb_protect_error` closure wrapper on `Mrb`.
//!
//! Inherent method that wraps mruby's `mrb_protect_error` so any
//! Ruby exception the body raises is caught and surfaced as
//! `Err(value)` instead of long-jumping past the Rust caller.

#[cfg(target_arch = "wasm32")]
use crate::{Mrb, Value};
#[cfg(target_arch = "wasm32")]
use mruby_sys as sys;

#[cfg(target_arch = "wasm32")]
impl Mrb {
    /// `mrb_protect_error(mrb, body, userdata, &error)` — run `body`
    /// inside a protected frame so any Ruby exception it raises is
    /// caught instead of long-jumping past the Rust call site. On
    /// success returns `Ok(value)` with the body's return value; on a
    /// raised exception returns `Err(exception_value)`.
    ///
    /// ## Closure form
    ///
    /// The closure receives a borrowed `&Mrb` (the same VM `self`
    /// points to) so it can call safe methods inside the protected
    /// frame without re-acquiring the borrow. It must return a
    /// `Value` — the protected frame's value is whatever the
    /// closure produces, mirroring mruby's own `body` contract.
    ///
    /// ## Drop semantics on the raise path
    ///
    /// When the closure raises, mruby long-jumps out of the body
    /// before the closure returns normally. Anything the closure
    /// captured that needs `Drop` to run (heap allocations, owned
    /// strings, etc.) **will not be dropped** on that path —
    /// `setjmp`/`longjmp` does not unwind Rust stack frames.
    ///
    /// **Capture `Copy` values only** (`Value` is `Copy`) unless
    /// the rare leak on the raise path is acceptable for the
    /// captured state. The closure-slot pattern below keeps the
    /// per-call overhead allocation-free; only the closure's own
    /// captures are at risk.
    pub fn protect<F>(&self, body: F) -> Result<Value, Value>
    where
        F: FnOnce(&Mrb) -> Value,
    {
        // Hold the closure in a stack-local Option so the trampoline
        // can `take()` it without owning a heap allocation. The
        // Option's storage outlives the FFI call by virtue of being a
        // local; on the raise path the long-jump leaves `slot` as
        // `None` (because the trampoline already took the closure
        // out) and the subsequent return into Rust drops it cleanly.
        let mut slot: Option<F> = Some(body);

        unsafe extern "C" fn trampoline<F>(
            mrb: *mut sys::mrb_state,
            userdata: *mut core::ffi::c_void,
        ) -> sys::mrb_value
        where
            F: FnOnce(&Mrb) -> Value,
        {
            // SAFETY: userdata is the `&mut Option<F>` from the
            // caller; mrb is the same live state passed to
            // mrb_protect_error.
            let slot: &mut Option<F> = unsafe { &mut *(userdata as *mut Option<F>) };
            let body = slot.take().expect("Mrb::protect trampoline invoked twice");
            let mrb_ref = unsafe { Mrb::borrow_raw(&mrb) };
            body(mrb_ref).into_raw()
        }

        let mut error: sys::mrb_bool = false;
        // SAFETY: `self` is alive; `trampoline::<F>` upholds the
        // `mrb_protect_error_func` ABI; `userdata` points to `slot`
        // on this stack frame which outlives the call. bindgen wraps
        // function-typedef parameters in `Option<…>`, so the
        // trampoline must be passed via `Some`.
        let ret = unsafe {
            sys::mrb_protect_error(
                self.as_ptr(),
                Some(trampoline::<F>),
                &mut slot as *mut Option<F> as *mut core::ffi::c_void,
                &mut error,
            )
        };
        let value = Value::from_raw(ret);
        if error {
            Err(value)
        } else {
            Ok(value)
        }
    }
}
