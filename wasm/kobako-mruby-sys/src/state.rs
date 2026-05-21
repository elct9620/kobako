//! RAII wrapper around mruby's `mrb_state *`.
//!
//! [`Mrb`] owns a freshly opened mruby VM. [`Mrb::open`] allocates a new
//! state via `mrb_open`; [`Drop`] releases it via `mrb_close`. Callers
//! that still reach for the raw FFI (during the staged migration) use
//! [`Mrb::as_ptr`] as an explicit escape hatch.
//!
//! `Mrb` is intentionally `!Send` and `!Sync` (inherited from
//! `NonNull<mrb_state>`): mruby's `mrb_state` is single-threaded and
//! must not cross thread boundaries.
//!
//! ## Why a newtype rather than passing `*mut mrb_state`
//!
//! Two problems with the raw pointer:
//!
//! 1. Every function that takes one must be `unsafe fn` even when it
//!    does nothing more than forward to FFI — "unsafe contagion" across
//!    every helper that touches the VM.
//! 2. Manual `mrb_close` calls scatter across every panic-outcome path
//!    in `__kobako_eval`. Forgetting one is a quiet memory leak the
//!    type system cannot catch.
//!
//! `Mrb` fixes both: the owning type makes "the VM is live" provable by
//! the borrow checker, and `Drop` makes `mrb_close` automatic.

#[cfg(target_arch = "wasm32")]
use crate as sys;
#[cfg(target_arch = "wasm32")]
use crate::Class;
#[cfg(target_arch = "wasm32")]
use crate::Value;
#[cfg(target_arch = "wasm32")]
use core::ptr::NonNull;

/// Owning handle to a live mruby VM. Closed automatically on drop.
///
/// On non-wasm32 targets the inner pointer field is absent because
/// [`Mrb::open`] always returns `Err` there; the type still compiles so
/// that `Result<Mrb, MrbOpenError>` is a uniform return type across
/// targets.
///
/// On wasm32 the type is `#[repr(transparent)]` over
/// `NonNull<mrb_state>` so [`Mrb::borrow_raw`] can fabricate a `&Mrb`
/// reference from a raw `*mut mrb_state` received at a C-bridge frame.
/// The two layouts are byte-identical there.
#[cfg_attr(target_arch = "wasm32", repr(transparent))]
pub struct Mrb {
    #[cfg(target_arch = "wasm32")]
    state: NonNull<sys::mrb_state>,
}

/// Returned by [`Mrb::open`] when `mrb_open` returns NULL (allocation
/// failure inside mruby) or on the host target where `mrb_open` is not
/// linked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MrbOpenError;

impl std::fmt::Display for MrbOpenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("mrb_open returned NULL")
    }
}

impl std::error::Error for MrbOpenError {}

impl Mrb {
    /// Open a fresh mruby state. Returns [`MrbOpenError`] when mruby's
    /// allocator cannot produce a state (or unconditionally on the host
    /// target — the mruby C API is not linked into the rlib).
    pub fn open() -> Result<Self, MrbOpenError> {
        #[cfg(target_arch = "wasm32")]
        {
            let raw = unsafe { sys::mrb_open() };
            NonNull::new(raw)
                .map(|state| Self { state })
                .ok_or(MrbOpenError)
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            Err(MrbOpenError)
        }
    }

    /// Raw `*mut mrb_state`. Use only at FFI boundaries that have not
    /// yet migrated to safe methods on `Mrb`. The returned pointer is
    /// valid for the lifetime of `&self`; callers must not call
    /// `mrb_close` on it (the `Mrb` Drop owns that). wasm32-only —
    /// host targets cannot construct an `Mrb`, so the raw-pointer
    /// escape hatch has no callers there.
    #[cfg(target_arch = "wasm32")]
    #[inline]
    pub fn as_ptr(&self) -> *mut sys::mrb_state {
        self.state.as_ptr()
    }

    /// Borrow a live `*mut mrb_state` as an `&Mrb` reference. Used by
    /// C-bridge frames that receive a raw pointer from mruby and need
    /// to call the safe [`Mrb`] methods without first acquiring an
    /// owning [`Mrb`].
    ///
    /// The returned reference does not own the state; no `mrb_close`
    /// runs when it goes out of scope. The owning `Mrb` (the one
    /// produced by [`Mrb::open`]) keeps Drop responsibility.
    ///
    /// # Safety
    ///
    /// `mrb` must point to a live mruby state that remains open for
    /// the lifetime `'a` of the returned borrow. Passing NULL is
    /// undefined behaviour. Sound only on wasm32 where `Mrb` is
    /// `#[repr(transparent)]` over `NonNull<mrb_state>`.
    #[cfg(target_arch = "wasm32")]
    #[inline]
    pub unsafe fn borrow_raw<'a>(mrb: *mut sys::mrb_state) -> &'a Mrb {
        debug_assert!(!mrb.is_null());
        // SAFETY: `Mrb` is `#[repr(transparent)]` over
        // `NonNull<mrb_state>`, which is itself `#[repr(transparent)]`
        // over `*mut mrb_state`. Casting `*mut mrb_state` to
        // `*const Mrb` produces a pointer with identical bit pattern.
        // Liveness for `'a` is upheld by the caller.
        unsafe { &*(mrb as *const Mrb) }
    }

    /// Return the currently pending mruby exception, or
    /// `mrb_nil_value()` (`w == 0`) if none. Reads `mrb->exc` via the
    /// layout-safe C accessor [`sys::kobako_get_exc`]; does NOT clear
    /// the field — callers pair this with [`Mrb::clear_exc`] after
    /// they have captured class/message/backtrace.
    #[cfg(target_arch = "wasm32")]
    pub fn pending_exc(&self) -> Value {
        // SAFETY: `self.state` is alive by the `&self` borrow.
        Value::from_raw(unsafe { sys::kobako_get_exc(self.as_ptr()) })
    }

    /// Clear `mrb->exc`. Idempotent; safe to call when no exception
    /// is pending. Used by the consumer crate's panic-recovery paths
    /// after the pending exception has been extracted, so subsequent
    /// mruby calls do not observe stale exception state.
    #[cfg(target_arch = "wasm32")]
    pub fn clear_exc(&self) {
        // SAFETY: `self.state` is alive by the `&self` borrow. The
        // return value (a `mrb_bool` snapshot of the prior `mrb->exc`
        // state) is intentionally discarded.
        let _ = unsafe { sys::mrb_check_error(self.as_ptr()) };
    }

    /// Return `mrb->object_class` as a typed [`Class`] handle.
    /// Replaces direct field access — the `object_class` field on
    /// the [`crate::mrb_state`] struct is `pub(crate)` so this
    /// accessor is the one external entry point. The free function
    /// [`crate::mrb_object_class`] remains for code paths that hold
    /// only a raw `*mut mrb_state` (currently the kobako-wasm
    /// install helpers).
    #[cfg(target_arch = "wasm32")]
    #[inline]
    pub fn object_class(&self) -> Class {
        // SAFETY: `self.state` is alive by the `&self` borrow.
        Class::from_raw(unsafe { sys::mrb_object_class(self.as_ptr()) })
    }

    // ----------------------------------------------------------------
    // String / Array / Hash factories.
    //
    // The mruby C API spells these as `mrb_str_new` / `mrb_ary_new` /
    // `mrb_hash_new` / `_set` / `_get` / `_keys`; the inherent methods
    // here keep the same names so call sites read with one-to-one
    // mapping to the C-side documentation. The `&Mrb` borrow upholds
    // liveness so each method is safe.
    // ----------------------------------------------------------------

    /// `mrb_str_new(mrb, p, len)` — construct an mruby `String` from
    /// `bytes`. The buffer is copied into the mruby heap; the slice
    /// only has to live for the duration of the call.
    ///
    /// `bytes.len()` saturates to [`i32::MAX`] (mruby's `mrb_int` on
    /// wasm32 is signed 32-bit). Real callers never reach that — the
    /// host-side String size cap (8 MiB) sits well below.
    #[cfg(target_arch = "wasm32")]
    #[inline]
    pub fn str_new(&self, bytes: &[u8]) -> Value {
        let len = bytes.len().min(i32::MAX as usize) as i32;
        // SAFETY: `self.state` is alive by the `&self` borrow; `bytes`
        // outlives the synchronous call.
        Value::from_raw(unsafe {
            sys::mrb_str_new(
                self.as_ptr(),
                bytes.as_ptr() as *const core::ffi::c_char,
                len,
            )
        })
    }

    /// `mrb_str_new_cstr(mrb, s)` — construct an mruby `String` from a
    /// NUL-terminated C string. The `&CStr` borrow guarantees the
    /// terminator.
    #[cfg(target_arch = "wasm32")]
    #[inline]
    pub fn str_new_cstr(&self, s: &core::ffi::CStr) -> Value {
        // SAFETY: `self.state` is alive; `s.as_ptr()` is
        // NUL-terminated by the `&CStr` contract.
        Value::from_raw(unsafe { sys::mrb_str_new_cstr(self.as_ptr(), s.as_ptr()) })
    }

    /// `mrb_ary_new(mrb)` — construct a fresh empty mruby `Array`.
    #[cfg(target_arch = "wasm32")]
    #[inline]
    pub fn ary_new(&self) -> Value {
        // SAFETY: `self.state` is alive.
        Value::from_raw(unsafe { sys::mrb_ary_new(self.as_ptr()) })
    }

    /// `mrb_ary_push(mrb, ary, val)` — append `val` to `ary`. `ary`
    /// must be an Array-tagged [`Value`] produced by the same VM.
    #[cfg(target_arch = "wasm32")]
    #[inline]
    pub fn ary_push(&self, ary: Value, val: Value) {
        // SAFETY: `self.state` is alive; both values originate from
        // the same VM by the single-VM contract.
        unsafe { sys::mrb_ary_push(self.as_ptr(), ary.as_raw(), val.as_raw()) };
    }

    /// `mrb_hash_new(mrb)` — construct a fresh empty mruby `Hash`.
    #[cfg(target_arch = "wasm32")]
    #[inline]
    pub fn hash_new(&self) -> Value {
        // SAFETY: `self.state` is alive.
        Value::from_raw(unsafe { sys::mrb_hash_new(self.as_ptr()) })
    }

    /// `mrb_hash_set(mrb, hash, key, val)` — assign `key => val` in
    /// `hash`.
    #[cfg(target_arch = "wasm32")]
    #[inline]
    pub fn hash_set(&self, hash: Value, key: Value, val: Value) {
        // SAFETY: as `ary_push`.
        unsafe { sys::mrb_hash_set(self.as_ptr(), hash.as_raw(), key.as_raw(), val.as_raw()) };
    }

    /// `mrb_hash_get(mrb, hash, key)` — return the value for `key`, or
    /// `nil` when absent.
    #[cfg(target_arch = "wasm32")]
    #[inline]
    pub fn hash_get(&self, hash: Value, key: Value) -> Value {
        // SAFETY: as `ary_push`.
        Value::from_raw(unsafe { sys::mrb_hash_get(self.as_ptr(), hash.as_raw(), key.as_raw()) })
    }

    /// `mrb_hash_keys(mrb, hash)` — return the Array of keys in
    /// `hash`.
    #[cfg(target_arch = "wasm32")]
    #[inline]
    pub fn hash_keys(&self, hash: Value) -> Value {
        // SAFETY: as `ary_push`.
        Value::from_raw(unsafe { sys::mrb_hash_keys(self.as_ptr(), hash.as_raw()) })
    }

    // ----------------------------------------------------------------
    // Symbol intern / lookup.
    // ----------------------------------------------------------------

    /// `mrb_intern_cstr(mrb, s)` — intern a NUL-terminated C string
    /// as a Symbol id.
    #[cfg(target_arch = "wasm32")]
    #[inline]
    pub fn intern_cstr(&self, s: &core::ffi::CStr) -> sys::mrb_sym {
        // SAFETY: `self.state` is alive; `s.as_ptr()` is
        // NUL-terminated by the `&CStr` contract.
        unsafe { sys::mrb_intern_cstr(self.as_ptr(), s.as_ptr()) }
    }

    /// `mrb_intern_str(mrb, str)` — intern the bytes of an mruby
    /// String value as a Symbol. Use this when the name arrives as
    /// arbitrary bytes that may not be NUL-safe; otherwise prefer
    /// [`Mrb::intern_cstr`].
    #[cfg(target_arch = "wasm32")]
    #[inline]
    pub fn intern_str(&self, s: Value) -> sys::mrb_sym {
        // SAFETY: `self.state` is alive; `s` originates from the same
        // VM.
        unsafe { sys::mrb_intern_str(self.as_ptr(), s.as_raw()) }
    }

    // ----------------------------------------------------------------
    // Top-level module / class registration.
    //
    // Each method mirrors the mruby C API one-to-one:
    //   - mrb_define_module(mrb, name)         -> define_module
    //   - mrb_define_class(mrb, name, super)   -> define_class
    //   - mrb_class_get(mrb, name)             -> class_get
    //   - mrb_define_global_const(mrb, n, v)   -> define_global_const
    //   - mrb_gv_set(mrb, sym, val)            -> gv_set
    //
    // The class lookup paths return a typed [`Class`]; consumers
    // check [`Class::is_null`] for "not found" the same way they
    // would on the C side.
    // ----------------------------------------------------------------

    /// `mrb_define_module(mrb, name)` — return the module named
    /// `name`, defining it at top level if not already present.
    #[cfg(target_arch = "wasm32")]
    #[inline]
    pub fn define_module(&self, name: &core::ffi::CStr) -> Class {
        // SAFETY: `self.state` is alive; `name` is NUL-terminated.
        Class::from_raw(unsafe { sys::mrb_define_module(self.as_ptr(), name.as_ptr()) })
    }

    /// `mrb_define_class(mrb, name, super_)` — define a top-level
    /// class named `name` inheriting from `super_`.
    #[cfg(target_arch = "wasm32")]
    #[inline]
    pub fn define_class(&self, name: &core::ffi::CStr, super_: Class) -> Class {
        // SAFETY: `self.state` is alive; `name` is NUL-terminated;
        // `super_` was produced by the same VM.
        Class::from_raw(unsafe {
            sys::mrb_define_class(self.as_ptr(), name.as_ptr(), super_.as_raw())
        })
    }

    /// `mrb_class_get(mrb, name)` — fetch the top-level class named
    /// `name`. The returned [`Class`] may be null when no such class
    /// is registered.
    #[cfg(target_arch = "wasm32")]
    #[inline]
    pub fn class_get(&self, name: &core::ffi::CStr) -> Class {
        // SAFETY: `self.state` is alive; `name` is NUL-terminated.
        Class::from_raw(unsafe { sys::mrb_class_get(self.as_ptr(), name.as_ptr()) })
    }

    /// `mrb_define_global_const(mrb, name, val)` — bind a top-level
    /// constant. Reachable as `name` and as `Object::name`.
    #[cfg(target_arch = "wasm32")]
    #[inline]
    pub fn define_global_const(&self, name: &core::ffi::CStr, val: Value) {
        // SAFETY: `self.state` is alive; `name` is NUL-terminated;
        // `val` originates from the same VM.
        unsafe { sys::mrb_define_global_const(self.as_ptr(), name.as_ptr(), val.as_raw()) };
    }

    /// `mrb_gv_set(mrb, sym, val)` — assign a global variable.
    #[cfg(target_arch = "wasm32")]
    #[inline]
    pub fn gv_set(&self, sym: sys::mrb_sym, val: Value) {
        // SAFETY: `self.state` is alive; `val` originates from the
        // same VM.
        unsafe { sys::mrb_gv_set(self.as_ptr(), sym, val.as_raw()) };
    }

    /// `mrb_sym_name(mrb, sym)` — return the C string name of `sym`,
    /// or `None` if mruby yields a NULL pointer (e.g. uninterned id).
    /// The returned slice points into mruby's interned string storage
    /// and lives for the duration of the VM.
    #[cfg(target_arch = "wasm32")]
    #[inline]
    pub fn sym_name(&self, sym: sys::mrb_sym) -> Option<&'static str> {
        // SAFETY: `self.state` is alive.
        let ptr = unsafe { sys::mrb_sym_name(self.as_ptr(), sym) };
        if ptr.is_null() {
            return None;
        }
        // SAFETY: mruby's interned symbol storage lives for the
        // duration of the VM; treating the slice as `'static` is
        // sound for that lifetime, which the caller upholds via the
        // owning `Mrb`.
        Some(
            unsafe { core::ffi::CStr::from_ptr(ptr) }
                .to_str()
                .unwrap_or(""),
        )
    }

    // ----------------------------------------------------------------
    // Protected execution.
    // ----------------------------------------------------------------

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
    /// [`Value`] — the protected frame's value is whatever the
    /// closure produces, mirroring mruby's own `body` contract.
    ///
    /// ## Drop semantics on the raise path
    ///
    /// When the closure raises, mruby long-jumps out of the body
    /// before the closure returns normally. Anything the closure
    /// captured that needs `Drop` to run (heap allocations, owned
    /// strings, etc.) **will not be dropped** on that path —
    /// `setjmp`/`longjmp` does not unwind Rust stack frames. Callers
    /// should keep captured state limited to `Copy` types
    /// ([`Value`] is `Copy`) or values whose leak on the rare raise
    /// path is acceptable. The closure-slot pattern below keeps the
    /// per-call overhead allocation-free; only the closure's own
    /// captures are at risk.
    #[cfg(target_arch = "wasm32")]
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
            let mrb_ref = unsafe { Mrb::borrow_raw(mrb) };
            body(mrb_ref).into_raw()
        }

        let mut error: sys::mrb_bool = 0;
        // SAFETY: `self.state` is alive; `trampoline::<F>` upholds
        // the `mrb_protect_error_func` ABI; `userdata` points to
        // `slot` on this stack frame which outlives the call.
        let ret = unsafe {
            sys::mrb_protect_error(
                self.as_ptr(),
                trampoline::<F>,
                &mut slot as *mut Option<F> as *mut core::ffi::c_void,
                &mut error,
            )
        };
        let value = Value::from_raw(ret);
        if error != 0 {
            Err(value)
        } else {
            Ok(value)
        }
    }
}

#[cfg(target_arch = "wasm32")]
impl Drop for Mrb {
    fn drop(&mut self) {
        // SAFETY: `state` was produced by `mrb_open` in `Mrb::open` and
        // has not been closed elsewhere — `as_ptr` hands out borrows but
        // never takes ownership.
        unsafe { sys::mrb_close(self.state.as_ptr()) };
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Drop for Mrb {
    fn drop(&mut self) {
        // Unreachable: `Mrb::open` always returns `Err` on host targets,
        // so no `Mrb` value can be constructed there. Required only so
        // the type satisfies `Drop` uniformly across targets.
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_returns_error_on_host_target() {
        // Host target: `mrb_open` is not linked, so `open` must yield
        // `Err` without attempting an FFI call. This is the documented
        // host-side contract; wasm32 coverage runs through the E2E
        // journeys.
        assert_eq!(Mrb::open().err(), Some(MrbOpenError));
    }
}
