//! Kobako runtime — installs the Kobako module surface onto an mruby VM
//! and owns the class handles needed by the dispatch layer.
//!
//! ## Why a separate type from [`crate::mruby::Mrb`]
//!
//! `Mrb` is the language-level VM owner: it knows how to open and close
//! an mruby state and nothing about kobako's own object surface. The
//! kobako-specific registrations (`Kobako` module, `Kobako::Transport`
//! namespace + `Proxy` abstract base, the `Kobako::Member` / `Kobako::Handle`
//! proxy subclasses, `Kobako::ServiceError` /
//! `Kobako::Transport::Error`, `Kernel#puts` / `Kernel#p` shims) belong to a
//! different concern and live behind this domain boundary.
//!
//! The shape mirrors `magnus::Ruby` for CRuby: a value-type "token" that
//! proves you can talk to the runtime, with no Drop and no lifetime —
//! liveness is the caller's contract, just as it is for mruby's own C
//! API. The C-bridges in [`crate::kobako::bridges`] remain
//! `unsafe extern "C" fn` callbacks invoked by mruby, but their bodies
//! acquire a [`Kobako`] through [`Kobako::resolve_raw`] and then call
//! safe methods.
//!
//! ## Lifecycle
//!
//! [`Kobako::install`] is called once per `__kobako_eval` invocation,
//! immediately after [`Mrb::open`]. It registers every boot-time entity
//! and returns a `Kobako` carrying the resolved class handles. The
//! returned value is then used to drive the Frame 1 preamble through
//! [`Kobako::install_groups`].
//!
//! C-bridges that receive a raw `*mut mrb_state` from mruby — the
//! [`crate::mruby::sys::mrb_func_t`] ABI mandates the raw pointer at
//! the bridge entry — use [`Kobako::resolve_raw`] to obtain the same
//! handle without repeating registration.

#[cfg(target_arch = "wasm32")]
pub(crate) mod bridges;
pub(crate) mod bytecode;
#[cfg(target_arch = "wasm32")]
mod codec_convert;
#[cfg(target_arch = "wasm32")]
mod install;
#[cfg(target_arch = "wasm32")]
pub(crate) mod io;

#[cfg(target_arch = "wasm32")]
use crate::mruby::sys;
#[cfg(target_arch = "wasm32")]
use crate::mruby::sys::Value;
#[cfg(target_arch = "wasm32")]
use crate::mruby::Mrb;
#[cfg(target_arch = "wasm32")]
use crate::transport::proxy::ExceptionPayload;

/// Mangled instance-variable name that `Kobako::Handle#initialize`
/// stores the Handle id under. Read back through [`Kobako::extract_handle_id`]
/// at every method dispatch — keeping the literal in a single
/// `const` makes the writer / reader pairing impossible to drift
/// silently when the ivar layout changes.
#[cfg(target_arch = "wasm32")]
const HANDLE_ID_IVAR: &core::ffi::CStr = c"@__kobako_id__";

/// Failures returned by [`Kobako::install_groups`] when a preamble entry
/// carries a name that cannot be passed through the mruby C API (which
/// expects NUL-terminated strings). wasm32-only because the preamble
/// install path itself is wasm32-only.
#[cfg(target_arch = "wasm32")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallGroupsError {
    /// A Group name contained an interior NUL byte.
    NulInGroupName,
    /// A Member name contained an interior NUL byte.
    NulInMemberName,
}

#[cfg(target_arch = "wasm32")]
impl std::fmt::Display for InstallGroupsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InstallGroupsError::NulInGroupName => {
                f.write_str("preamble Group name contains interior NUL byte")
            }
            InstallGroupsError::NulInMemberName => {
                f.write_str("preamble Member name contains interior NUL byte")
            }
        }
    }
}

#[cfg(target_arch = "wasm32")]
impl std::error::Error for InstallGroupsError {}

/// Handle to a Kobako runtime installed on a live mruby VM.
///
/// `Kobako` is a value-type token: it carries the raw `*mut mrb_state`
/// pointer plus the resolved class handles, but does not own the VM —
/// the caller is responsible for keeping the underlying state live for
/// the duration of any `Kobako` method call. Constructed through one of
/// two entry points:
///
///   * [`Kobako::install`] — register every boot-time entity then
///     return a fully populated handle. Takes an [`Mrb`] borrow so the
///     pipeline below it stays in safe Rust.
///   * [`Kobako::resolve_raw`] — re-resolve class handles produced by
///     a prior install. Used by C-bridges, which mruby invokes with a
///     raw `*mut mrb_state` per the [`crate::mruby::sys::mrb_func_t`]
///     ABI; the raw entry is mandated by mruby, not by kobako.
///
/// ## wasm32-only
///
/// `Kobako` and its methods only exist on `wasm32-wasip1` (the
/// production target). Host-target `cargo test` does not link
/// `libmruby.a`, so neither [`Mrb::open`] nor `Kobako::install` is
/// callable there; the type is cfg-gated out entirely.
#[cfg(target_arch = "wasm32")]
pub struct Kobako {
    mrb: *mut sys::mrb_state,
    /// `Kobako::Member` base class — parent of every bound Member proxy
    /// installed via [`Kobako::install_groups`].
    member_class: sys::Class,
    handle_class: sys::Class,
    service_error_class: sys::Class,
    transport_error_class: sys::Class,
}

// The canonical mruby `nil` / `true` / `false` value snapshots no
// longer live on the `Kobako` struct. They are captured once into
// the sys-side [`Value`] immediates cache and read via
// `Value::nil()` / `Value::true_()` / `Value::false_()` — each call
// is a single atomic load against the `OnceLock`, on par with the
// previous per-instance field read.

#[cfg(target_arch = "wasm32")]
impl Kobako {
    /// Install the Kobako runtime onto `mrb` and return a handle to
    /// the resulting class registrations. Order matters:
    /// +install_kernel_delegators+ looks up `$stdout` / `$stderr` at
    /// call time, so +install_io_globals+ must wire those globals
    /// first.
    pub fn install(mrb: &Mrb) -> Self {
        let classes = install::install_kobako_classes(mrb);
        install::install_io_globals(mrb);
        install::install_kernel_delegators(mrb);

        Self {
            mrb: mrb.as_ptr(),
            member_class: classes.member_class,
            handle_class: classes.handle_class,
            service_error_class: classes.service_error_class,
            transport_error_class: classes.transport_error_class,
        }
    }

    /// Resolve the class handles produced by a prior install, from a
    /// raw `*mut mrb_state`. C-bridge re-entry point — mruby invokes
    /// bridges with a raw `*mut mrb_state` per the
    /// [`crate::mruby::sys::mrb_func_t`] ABI, and this is how those
    /// bridge bodies recover the `Kobako` handle.
    ///
    /// # Safety
    ///
    /// `mrb` must be a live mruby state on which [`Kobako::install`]
    /// has already run. Calling this against a fresh state without
    /// prior install would surface as a NULL `mrb_class_get_under`
    /// return value and later UB; the C-bridge entry points uphold
    /// the install precondition by construction (they are invoked
    /// through registrations done at install time).
    pub unsafe fn resolve_raw(mrb: *mut sys::mrb_state) -> Self {
        // SAFETY: `mrb` is live by the function's safety contract.
        // `mrb_define_module` is idempotent (returns the existing
        // module if already registered); `class_get_under` returns
        // the already-registered class produced by `install`.
        let mrb_ref = unsafe { Mrb::borrow_raw(&mrb) };
        let kobako_mod = mrb_ref.define_module(c"Kobako");
        let transport_mod = kobako_mod.define_module_under(mrb_ref, c"Transport");
        let member_class = kobako_mod.class_get_under(mrb_ref, c"Member");
        let handle_class = kobako_mod.class_get_under(mrb_ref, c"Handle");
        let service_error_class = kobako_mod.class_get_under(mrb_ref, c"ServiceError");
        let transport_error_class = transport_mod.class_get_under(mrb_ref, c"Error");
        Self {
            mrb,
            member_class,
            handle_class,
            service_error_class,
            transport_error_class,
        }
    }

    /// Install Namespace / Member proxy classes from a Frame 1
    /// preamble. Each Group becomes a top-level Ruby module; each Member
    /// becomes a subclass of `Kobako::Member` under its Namespace so the
    /// singleton-class `method_missing` shim is inherited.
    pub fn install_groups(
        &self,
        preamble: &[(String, Vec<String>)],
    ) -> Result<(), InstallGroupsError> {
        let mrb = self.mrb();
        for (group_name, members) in preamble {
            let group_cstr = std::ffi::CString::new(group_name.as_str())
                .map_err(|_| InstallGroupsError::NulInGroupName)?;
            let group_mod = mrb.define_module(&group_cstr);
            for member_name in members {
                let member_cstr = std::ffi::CString::new(member_name.as_str())
                    .map_err(|_| InstallGroupsError::NulInMemberName)?;
                group_mod.define_class_under(mrb, &member_cstr, self.member_class);
            }
        }
        Ok(())
    }

    /// Raise `Kobako::Transport::Error` with `msg`. Diverges — `mrb_raise` does
    /// not return.
    ///
    /// # Safety
    ///
    /// Only callable from contexts that mruby may unwind from (C
    /// bridges, mrb_funcall handlers). Calling from arbitrary Rust code
    /// would jump through mruby's exception machinery in a way the Rust
    /// stack does not anticipate.
    pub unsafe fn raise_transport_error(&self, msg: &core::ffi::CStr) -> ! {
        // SAFETY: bridge frame — caller upholds the unwind contract.
        unsafe { self.transport_error_class.raise(self.mrb(), msg) };
    }

    /// Raise `Kobako::ServiceError` for `ex`. Diverges — `mrb_raise`
    /// does not return.
    ///
    /// SPEC.md § Error Classes (governing) + docs/wire-contract.md
    /// § Fault Envelope pin every Response.err `type` value to the
    /// single guest-side `Kobako::ServiceError` class.
    ///
    /// # Safety
    ///
    /// As [`Kobako::raise_transport_error`].
    pub unsafe fn raise_service_error(&self, ex: &ExceptionPayload) -> ! {
        let msg = std::ffi::CString::new(ex.message.as_str()).unwrap_or_default();
        // SAFETY: bridge frame — caller upholds the unwind contract.
        unsafe { self.service_error_class.raise(self.mrb(), &msg) };
    }

    // ----------------------------------------------------------------
    // VM access. The +mrb+ accessor synthesises a borrowed [`Mrb`]
    // reference over the raw pointer so callers can use the safe
    // builder / accessor methods (`hash_get`, `intern_cstr`, etc.)
    // without each method re-implementing the same FFI dispatch.
    // ----------------------------------------------------------------

    /// Borrow `self.mrb` as `&Mrb`. The borrow lives for the duration
    /// of `&self`, which the [`Kobako`] construction contract ties
    /// to the underlying `mrb_state`'s liveness.
    #[inline]
    pub(crate) fn mrb(&self) -> &Mrb {
        // SAFETY: `Kobako` is only constructed against a live
        // `mrb_state` (via `install` / `resolve_raw`), and the caller
        // upholds liveness for the duration of any method call on it.
        unsafe { Mrb::borrow_raw(&self.mrb) }
    }

    // ----------------------------------------------------------------
    // Collection / hash / handle helpers.
    // ----------------------------------------------------------------

    /// Return the number of elements in an mruby Array or Hash by
    /// calling `.length` and unboxing the resulting Fixnum directly.
    /// Used wherever a collection length is needed without a direct
    /// FFI binding for `mrb_ary_len`. Returns 0 when `.length` does
    /// not yield a Fixnum — the mruby core implementations always do,
    /// so non-Fixnum here signals a user-overridden +length+ returning
    /// nonsense; preserving the previous +.unwrap_or(0)+ semantics so
    /// callers see "empty collection" rather than a panic.
    pub fn collection_len(&self, col: Value) -> usize {
        use crate::mruby::sys::FromValue;
        let len_val = col.call(self.mrb(), c"length", &[]);
        let Some(len) = i32::from_value(len_val) else {
            return 0;
        };
        if len < 0 {
            0
        } else {
            len as usize
        }
    }

    /// Collect `exc_val.backtrace` (an mruby `Array of String`) into a
    /// Rust `Vec<String>`. Used by the guest panic path
    /// (`crate::abi::eval` / `crate::abi::run`) to populate the Panic
    /// envelope's `backtrace` field
    /// (docs/wire-codec.md § Panic Envelope).
    ///
    /// mruby's default build keeps the backtrace, so `.backtrace`
    /// returns an Array of String. If the runtime is ever rebuilt
    /// without keep-mode the call yields a non-Array value (typically
    /// `nil`); fall back to an empty vec so the Panic envelope still
    /// serializes cleanly.
    pub fn extract_backtrace(&self, exc_val: Value) -> Vec<String> {
        let bt_val = exc_val.call(self.mrb(), c"backtrace", &[]);
        if bt_val.classname(self.mrb()) != "Array" {
            return Vec::new();
        }
        // SAFETY: classname check above proves Array-tagged.
        let bt_ary = unsafe { sys::Array::from_value_unchecked(bt_val) };
        let len = self.collection_len(bt_val);
        let mut lines = Vec::with_capacity(len);
        for i in 0..len {
            lines.push(bt_ary.entry(i as i32).to_string(self.mrb()));
        }
        lines
    }

    /// Snapshot every top-level constant currently defined on `Object`
    /// by calling `Object.constants` and unpacking the returned Symbol
    /// Array into a `Vec<String>`. Used by `__kobako_run` to compute
    /// the E-27 `details:` payload: a baseline taken after kobako
    /// install + preamble materialise (before snippet replay) is
    /// subtracted from a post-replay snapshot, yielding the constants
    /// the preloaded snippets contributed (docs/behavior.md B-31 / E-27).
    ///
    /// Returns an empty vec when `Object.constants` does not return an
    /// Array — Ruby core guarantees it does, but the defensive fallback
    /// matches [`Self::extract_backtrace`]'s style and keeps the Panic
    /// envelope serialising cleanly under guest-class shenanigans.
    pub fn top_level_constants(&self) -> Vec<String> {
        // SAFETY: `mrb->object_class` lives until `mrb_close`; the
        // shim behind `Class::as_value` reuses mruby's own boxing
        // logic.
        let object_value = unsafe { self.mrb().object_class().as_value(self.mrb()) };
        let consts = object_value.call(self.mrb(), c"constants", &[]);
        if consts.classname(self.mrb()) != "Array" {
            return Vec::new();
        }
        // SAFETY: classname check above proves Array-tagged.
        let consts_ary = unsafe { sys::Array::from_value_unchecked(consts) };
        let len = self.collection_len(consts);
        let mut names = Vec::with_capacity(len);
        for i in 0..len {
            names.push(consts_ary.entry(i as i32).to_string(self.mrb()));
        }
        names
    }

    /// Store `id_val` into a fresh `Kobako::Handle` instance's
    /// `@__kobako_id__` ivar. Used by the `Kobako::Handle#initialize`
    /// C bridge.
    pub fn set_handle_id(&self, target: Value, id_val: Value) {
        let sym = self.mrb().intern_cstr(HANDLE_ID_IVAR);
        target.iv_set(self.mrb(), sym, id_val);
    }

    /// Read the `u32` Handle id stored in a `Kobako::Handle` instance's
    /// `@__kobako_id__` instance variable. Returns 0 when the ivar is
    /// missing, not a Fixnum, or carries a negative payload — the
    /// resolver downstream treats id 0 as undefined per
    /// docs/behavior.md B-19. The previous +.to_s.parse.unwrap_or(0)+
    /// path was lossy at the upper boundary (Fixnum > i32::MAX would
    /// silently truncate) and round-tripped through the mruby string
    /// machinery on every dispatch; the direct unbox is both faster
    /// and tighter on the wire-violation surface.
    pub fn extract_handle_id(&self, handle_val: Value) -> u32 {
        let id_sym = self.mrb().intern_cstr(HANDLE_ID_IVAR);
        use crate::mruby::sys::FromValue;
        let id_val = handle_val.iv_get(self.mrb(), id_sym);
        let Some(id) = i32::from_value(id_val) else {
            return 0;
        };
        if id < 0 {
            0
        } else {
            id as u32
        }
    }
}
