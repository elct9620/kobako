//! Kobako runtime — installs the Kobako module surface onto an mruby VM
//! and owns the class handles needed by the dispatch layer.
//!
//! ## Why a separate type from [`crate::mruby::Mrb`]
//!
//! `Mrb` is the language-level VM owner: it knows how to open and close
//! an mruby state and nothing about kobako's own object surface. The
//! kobako-specific registrations (`Kobako` module, `Kobako::RPC` base
//! class, `Kobako::RPC::Handle`, `Kobako::ServiceError` /
//! `Kobako::RPC::WireError`, `Kernel#puts` / `Kernel#p` shims) belong to a
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
//! C-bridges that receive a raw `*mut mrb_state` from mruby use the
//! [`Kobako::resolve_raw`] entry to obtain the same handle without
//! repeating registration.

#[cfg(any(target_arch = "wasm32", test))]
pub(crate) mod bridges;
pub(crate) mod bytecode;
#[cfg(target_arch = "wasm32")]
mod install;
#[cfg(any(target_arch = "wasm32", test))]
pub(crate) mod io;
#[cfg(target_arch = "wasm32")]
mod wire_convert;

#[cfg(target_arch = "wasm32")]
use crate::mruby::cstr_ptr;
use crate::mruby::sys;
#[cfg(target_arch = "wasm32")]
use crate::mruby::sys::Value;
#[cfg(target_arch = "wasm32")]
use crate::mruby::Mrb;
#[cfg(target_arch = "wasm32")]
use crate::rpc::client::ExceptionPayload;

// --------------------------------------------------------------------
// C-string constants — NUL-terminated names passed to the mruby C API.
// Bundled into one wasm32-gated module so the per-const cfg attribute
// does not multiply across the surface.
// --------------------------------------------------------------------

#[cfg(target_arch = "wasm32")]
use names::*;

#[cfg(target_arch = "wasm32")]
pub(crate) mod names {
    pub const KOBAKO_NAME: &[u8] = b"Kobako\0";
    pub const RPC_NAME: &[u8] = b"RPC\0";
    pub const CLIENT_NAME: &[u8] = b"Client\0";
    pub const HANDLE_NAME: &[u8] = b"Handle\0";
    pub const METHOD_MISSING_NAME: &[u8] = b"method_missing\0";
    pub const RESPOND_TO_MISSING_NAME: &[u8] = b"respond_to_missing?\0";
    pub const INITIALIZE_NAME: &[u8] = b"initialize\0";
    pub const SERVICE_ERROR_NAME: &[u8] = b"ServiceError\0";
    pub const DISCONNECTED_NAME: &[u8] = b"Disconnected\0";
    pub const RUNTIME_ERROR_NAME: &[u8] = b"RuntimeError\0";
    pub const WIRE_ERROR_NAME: &[u8] = b"WireError\0";
    pub const BYTECODE_ERROR_NAME: &[u8] = b"BytecodeError\0";
    pub const IO_NAME: &[u8] = b"IO\0";
    pub const STDOUT_CONST_NAME: &[u8] = b"STDOUT\0";
    pub const STDERR_CONST_NAME: &[u8] = b"STDERR\0";
    pub const STDOUT_GVAR_NAME: &[u8] = b"$stdout\0";
    pub const STDERR_GVAR_NAME: &[u8] = b"$stderr\0";
    pub const MODE_WRITE: &[u8] = b"w\0";
}

/// SPEC § Error Classes mapping from a Response.err `type` field to a
/// guest-side mruby class. Routed through
/// [`service_error_class_for_kind`] so the decision can be exercised
/// from host-target `cargo test`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ServiceErrorClass {
    /// `Kobako::ServiceError::Disconnected`. Mapped from `type =
    /// "disconnected"` — the only named subclass today.
    Disconnected,
    /// `Kobako::ServiceError`. Any other reserved or future `type`
    /// value lands on the parent class.
    Base,
}

/// Pure mapping from the Response.err `type` field to the
/// [`ServiceErrorClass`] used by [`Kobako::raise_service_error`].
pub(crate) fn service_error_class_for_kind(kind: &str) -> ServiceErrorClass {
    if kind == "disconnected" {
        ServiceErrorClass::Disconnected
    } else {
        ServiceErrorClass::Base
    }
}

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
/// the three entry points:
///
///   * [`Kobako::install`] / [`Kobako::install_raw`] — register every
///     boot-time entity then return a fully populated handle.
///   * [`Kobako::resolve_raw`] — re-resolve class handles produced by a
///     prior install (used by C-bridges).
///
/// ## Dual-target layout
///
/// On `wasm32-wasip1` (the production target), `Kobako` carries the raw
/// mruby state pointer and five class handles produced by
/// `install_raw`. On every other target (used for `cargo test` on the
/// developer's machine, where `libmruby.a` is not linked), `Kobako` is
/// an empty braced struct — the install/resolve methods short-circuit
/// to `Self {}` and the FFI-routed methods are cfg-gated out.
///
/// This split keeps the type's name and uniform method shape available
/// on both targets so the host-side `install_raw_is_safe_no_op_on_host`
/// test can exercise the cfg gate without an mruby link.
#[cfg(target_arch = "wasm32")]
pub struct Kobako {
    mrb: *mut sys::mrb_state,
    /// `Kobako::RPC::Client` base class — parent of every Member
    /// installed via [`Kobako::install_groups`].
    client_class: sys::Class,
    handle_class: sys::Class,
    service_error_class: sys::Class,
    disconnected_class: sys::Class,
    wire_error_class: sys::Class,
}

// The canonical mruby `nil` / `true` / `false` value snapshots no
// longer live on the `Kobako` struct. They are captured once into
// the sys-side [`Value`] immediates cache and read via
// `Value::nil()` / `Value::true_()` / `Value::false_()` — each call
// is a single atomic load against the `OnceLock`, on par with the
// previous per-instance field read.

/// Host-target stub for [`Kobako`]. See the wasm32 declaration above
/// for the production-target shape and field-level docs.
#[cfg(not(target_arch = "wasm32"))]
pub struct Kobako {}

impl Kobako {
    /// Install the Kobako runtime onto `mrb` and return a handle to the
    /// resulting class registrations. Safe wrapper over
    /// [`Kobako::install_raw`]. wasm32-only — host callers do not need
    /// this entry; the `install_raw_is_safe_no_op_on_host` test reaches
    /// straight for [`Kobako::install_raw`].
    #[cfg(target_arch = "wasm32")]
    pub fn install(mrb: &Mrb) -> Self {
        // SAFETY: `mrb` is a live, non-closed state per the `&Mrb`
        // borrow.
        unsafe { Self::install_raw(mrb.as_ptr()) }
    }

    /// Install the Kobako runtime against a raw `*mut mrb_state`.
    ///
    /// # Safety
    ///
    /// `mrb` must be a live mruby state — not null, not yet closed, and
    /// not concurrently mutated. Intended for entry points that receive
    /// a raw pointer from mruby itself; the safe wrapper
    /// [`Kobako::install`] is the preferred entry from owning Rust code.
    #[cfg_attr(not(target_arch = "wasm32"), allow(unused_variables))]
    pub unsafe fn install_raw(mrb: *mut sys::mrb_state) -> Self {
        #[cfg(target_arch = "wasm32")]
        {
            // Stage the install across three step helpers so the
            // monolithic 150-line `unsafe` block becomes three
            // localised ones, each with its own SAFETY note. Order
            // matters: install_kernel_delegators looks up `$stdout`
            // / `$stderr` at call time, so install_io_globals must
            // wire those globals first.
            // SAFETY: `mrb` is live by the function's safety
            // contract; each helper documents its own preconditions.
            let classes = unsafe { install::install_kobako_classes(mrb) };
            unsafe { install::install_io_globals(mrb) };
            unsafe { install::install_kernel_delegators(mrb) };

            Self {
                mrb,
                client_class: classes.client_class,
                handle_class: classes.handle_class,
                service_error_class: classes.service_error_class,
                disconnected_class: classes.disconnected_class,
                wire_error_class: classes.wire_error_class,
            }
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            Self {}
        }
    }

    /// Resolve the class handles produced by a prior install, from a
    /// raw `*mut mrb_state`. wasm32-only — the host stub of [`Kobako`]
    /// is exercised exclusively through [`Kobako::install_raw`], so the
    /// C-bridge re-entry point does not need a host counterpart.
    ///
    /// # Safety
    ///
    /// `mrb` must be a live mruby state on which [`Kobako::install`] /
    /// [`Kobako::install_raw`] has already run. Calling this against a
    /// fresh state without prior install would surface as a NULL
    /// `mrb_class_get_under` return value and later UB; the C-bridge
    /// entry points uphold the install precondition by construction
    /// (they are invoked through registrations done at install time).
    #[cfg(target_arch = "wasm32")]
    pub unsafe fn resolve_raw(mrb: *mut sys::mrb_state) -> Self {
        {
            // SAFETY of every FFI call below: `mrb` is live by the
            // function's safety contract; `mrb_define_module` is
            // idempotent (returns the existing module if already
            // registered); `mrb_class_get_under` returns the
            // already-registered class produced by `install_raw`.
            unsafe {
                let kobako_mod = sys::mrb_define_module(mrb, cstr_ptr(KOBAKO_NAME));
                let rpc_mod = sys::mrb_define_module_under(mrb, kobako_mod, cstr_ptr(RPC_NAME));
                let client_class = sys::Class::from_raw(sys::mrb_class_get_under(
                    mrb,
                    rpc_mod,
                    cstr_ptr(CLIENT_NAME),
                ));
                let handle_class = sys::Class::from_raw(sys::mrb_class_get_under(
                    mrb,
                    rpc_mod,
                    cstr_ptr(HANDLE_NAME),
                ));
                let service_error_class = sys::Class::from_raw(sys::mrb_class_get_under(
                    mrb,
                    kobako_mod,
                    cstr_ptr(SERVICE_ERROR_NAME),
                ));
                let disconnected_class = sys::Class::from_raw(sys::mrb_class_get_under(
                    mrb,
                    service_error_class.as_raw(),
                    cstr_ptr(DISCONNECTED_NAME),
                ));
                let wire_error_class = sys::Class::from_raw(sys::mrb_class_get_under(
                    mrb,
                    rpc_mod,
                    cstr_ptr(WIRE_ERROR_NAME),
                ));
                Self {
                    mrb,
                    client_class,
                    handle_class,
                    service_error_class,
                    disconnected_class,
                    wire_error_class,
                }
            }
        }
    }

    /// Install Namespace / Member proxy classes from a Frame 1
    /// preamble. Each Group becomes a top-level Ruby module; each Member
    /// becomes a subclass of `Kobako::RPC::Client` under its Namespace so the
    /// singleton-class `method_missing` shim is inherited. wasm32-only —
    /// host callers do not drive Frame 1 preamble.
    #[cfg(target_arch = "wasm32")]
    pub fn install_groups(
        &self,
        preamble: &[(String, Vec<String>)],
    ) -> Result<(), InstallGroupsError> {
        {
            for (group_name, members) in preamble {
                let group_cstr = std::ffi::CString::new(group_name.as_str())
                    .map_err(|_| InstallGroupsError::NulInGroupName)?;
                // SAFETY: `self.mrb` is alive; group name is NUL-terminated.
                let group_mod = unsafe { sys::mrb_define_module(self.mrb, group_cstr.as_ptr()) };
                for member_name in members {
                    let member_cstr = std::ffi::CString::new(member_name.as_str())
                        .map_err(|_| InstallGroupsError::NulInMemberName)?;
                    // SAFETY: as above; `self.client_class` was produced by
                    // install_raw.
                    unsafe {
                        sys::mrb_define_class_under(
                            self.mrb,
                            group_mod,
                            member_cstr.as_ptr(),
                            self.client_class.as_raw(),
                        )
                    };
                }
            }
            Ok(())
        }
    }

    /// Raise `Kobako::RPC::WireError` with `msg`. Diverges — `mrb_raise` does
    /// not return.
    ///
    /// # Safety
    ///
    /// Only callable from contexts that mruby may unwind from (C
    /// bridges, mrb_funcall handlers). Calling from arbitrary Rust code
    /// would jump through mruby's exception machinery in a way the Rust
    /// stack does not anticipate.
    #[cfg(target_arch = "wasm32")]
    pub unsafe fn raise_wire_error(&self, msg: &[u8]) -> ! {
        sys::mrb_raise(self.mrb, self.wire_error_class.as_raw(), cstr_ptr(msg));
    }

    /// Raise the matching `Kobako::ServiceError` subclass for `ex`.
    /// Diverges — `mrb_raise` does not return.
    ///
    /// SPEC.md § Error Classes (governing) + docs/wire-contract.md
    /// § Fault Envelope pin the mapping from the Response.err `type`
    /// field to a guest-side mruby class. The mapping itself lives in
    /// the pure helper [`service_error_class_for_kind`] so the routing
    /// can be exercised from `cargo test` without the FFI surface.
    ///
    /// # Safety
    ///
    /// As [`Kobako::raise_wire_error`].
    #[cfg(target_arch = "wasm32")]
    pub unsafe fn raise_service_error(&self, ex: &ExceptionPayload) -> ! {
        let target_cls = match service_error_class_for_kind(&ex.kind) {
            ServiceErrorClass::Disconnected => self.disconnected_class,
            ServiceErrorClass::Base => self.service_error_class,
        };
        let msg = std::ffi::CString::new(ex.message.as_str()).unwrap_or_default();
        sys::mrb_raise(self.mrb, target_cls.as_raw(), msg.as_ptr());
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
    #[cfg(target_arch = "wasm32")]
    #[inline]
    pub(crate) fn mrb(&self) -> &Mrb {
        // SAFETY: `Kobako` is only constructed against a live
        // `mrb_state` (via `install_raw` / `resolve_raw`), and the
        // caller upholds liveness for the duration of any method
        // call on it.
        unsafe { Mrb::borrow_raw(self.mrb) }
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
    #[cfg(target_arch = "wasm32")]
    pub fn collection_len(&self, col: Value) -> usize {
        let len_val = col.call(self.mrb(), c"length", &[]);
        if !len_val.is_integer() {
            return 0;
        }
        // SAFETY: gated by the is_integer check above.
        let len = unsafe { len_val.unbox_integer() };
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
    #[cfg(target_arch = "wasm32")]
    pub fn extract_backtrace(&self, exc_val: Value) -> Vec<String> {
        let bt_val = exc_val.call(self.mrb(), c"backtrace", &[]);
        if bt_val.classname(self.mrb()) != "Array" {
            return Vec::new();
        }
        let len = self.collection_len(bt_val);
        let mut lines = Vec::with_capacity(len);
        for i in 0..len {
            // SAFETY: +bt_val+ is Array-tagged by the classname check
            // above; +i+ stays in range by the +len+ bound.
            let line = unsafe { bt_val.ary_entry(i as i32) };
            lines.push(line.to_string(self.mrb()));
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
    #[cfg(target_arch = "wasm32")]
    pub fn top_level_constants(&self) -> Vec<String> {
        // SAFETY: the shim turns +mrb->object_class+ (which lives
        // until +mrb_close+) into the canonical class-tagged Value.
        let object_value =
            Value::from_raw(unsafe { sys::kobako_class_value(sys::mrb_object_class(self.mrb)) });
        let consts = object_value.call(self.mrb(), c"constants", &[]);
        if consts.classname(self.mrb()) != "Array" {
            return Vec::new();
        }
        let len = self.collection_len(consts);
        let mut names = Vec::with_capacity(len);
        for i in 0..len {
            // SAFETY: consts is Array-tagged by the classname check;
            // ary_entry stays in range by the +len+ bound.
            let entry = unsafe { consts.ary_entry(i as i32) };
            names.push(entry.to_string(self.mrb()));
        }
        names
    }

    /// Store `id_val` into a fresh `Kobako::RPC::Handle` instance's
    /// `@__kobako_id__` ivar. Used by the `Kobako::RPC::Handle#initialize`
    /// C bridge.
    #[cfg(target_arch = "wasm32")]
    pub fn set_handle_id(&self, target: Value, id_val: Value) {
        let sym = self.mrb().intern_cstr(c"@__kobako_id__");
        target.iv_set(self.mrb(), sym, id_val);
    }

    /// Read the `u32` Handle id stored in a `Kobako::RPC::Handle` instance's
    /// `@__kobako_id__` instance variable. Returns 0 when the ivar is
    /// missing, not a Fixnum, or carries a negative payload — the
    /// resolver downstream treats id 0 as undefined per
    /// docs/behavior.md B-19. The previous +.to_s.parse.unwrap_or(0)+
    /// path was lossy at the upper boundary (Fixnum > i32::MAX would
    /// silently truncate) and round-tripped through the mruby string
    /// machinery on every dispatch; the direct unbox is both faster
    /// and tighter on the wire-violation surface.
    #[cfg(target_arch = "wasm32")]
    pub fn extract_handle_id(&self, handle_val: Value) -> u32 {
        let id_sym = self.mrb().intern_cstr(c"@__kobako_id__");
        let id_val = handle_val.iv_get(self.mrb(), id_sym);
        if !id_val.is_integer() {
            return 0;
        }
        // SAFETY: gated by the is_integer check above.
        let id = unsafe { id_val.unbox_integer() };
        if id < 0 {
            0
        } else {
            id as u32
        }
    }

    // ----------------------------------------------------------------
    // RPC dispatch.
    // ----------------------------------------------------------------

    /// Invoke `invoke_rpc` and convert the result to a [`Value`]. On
    /// `Response.err`, raises a matching `Kobako::ServiceError`
    /// subclass; on any other wire-layer fault, raises `Kobako::RPC::WireError`
    /// with `wire_err_msg`. Both raise paths diverge — `mrb_raise` does
    /// not return.
    #[cfg(target_arch = "wasm32")]
    pub fn dispatch_invoke(
        &self,
        target: crate::rpc::envelope::Target,
        method_name: &str,
        wire_args: &[crate::codec::Value],
        wire_kwargs: &[(String, crate::codec::Value)],
        wire_err_msg: &[u8],
    ) -> Value {
        use crate::rpc::client::invoke_rpc;
        match invoke_rpc(target, method_name, wire_args, wire_kwargs) {
            Ok(wire_val) => self.to_mrb_value(wire_val),
            Err(crate::rpc::client::InvokeError::Service(ex)) => {
                // SAFETY: bridge frame — mruby will unwind through
                // `mrb_raise`.
                unsafe { self.raise_service_error(&ex) };
            }
            Err(_) => {
                // SAFETY: as above.
                unsafe { self.raise_wire_error(wire_err_msg) };
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_raw_is_safe_no_op_on_host() {
        // On host target the `install_raw` body short-circuits via the
        // `target_arch = "wasm32"` cfg, so passing a null `mrb` is safe.
        // This guard documents the host-side contract: the function
        // exists with a stable signature and is a true no-op when the
        // FFI cannot reach mruby.
        unsafe {
            let _ = Kobako::install_raw(core::ptr::null_mut());
        }
    }

    #[test]
    fn service_error_class_routes_disconnected_to_subclass() {
        assert_eq!(
            service_error_class_for_kind("disconnected"),
            ServiceErrorClass::Disconnected
        );
    }

    #[test]
    fn service_error_class_routes_unmapped_kinds_to_base() {
        // SPEC reserves "runtime", "argument", "undefined", "type" plus
        // any future value; all must land on the base class until a
        // dedicated guest-side subclass is registered for them.
        for kind in ["runtime", "argument", "undefined", "type", "future_type"] {
            assert_eq!(
                service_error_class_for_kind(kind),
                ServiceErrorClass::Base,
                "unmapped kind {kind:?} must fall back to ServiceError base"
            );
        }
    }

    #[test]
    fn service_error_class_treats_empty_kind_as_base() {
        // A Response.err with an empty `type` is a wire violation, but
        // the routing must still land on a real class rather than
        // panic; SPEC pins "any unmapped value → base" with no
        // empty-string carve-out.
        assert_eq!(service_error_class_for_kind(""), ServiceErrorClass::Base);
    }
}
