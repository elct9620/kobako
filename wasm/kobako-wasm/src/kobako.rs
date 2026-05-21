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
use crate::cstr;
#[cfg(target_arch = "wasm32")]
use crate::mruby::cstr_ptr;
use crate::mruby::sys;
#[cfg(target_arch = "wasm32")]
use crate::mruby::Mrb;
#[cfg(target_arch = "wasm32")]
use crate::mruby::MrbValueExt;
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
    /// `b"@__kobako_id__\0"` — mangled instance-variable name that
    /// `Kobako::RPC::Handle#initialize` stores the Handle id under. Used by
    /// the handle-id setter / getter on [`super::Kobako`].
    pub const HANDLE_ID_IVAR: &[u8] = b"@__kobako_id__\0";
    pub const IO_NAME: &[u8] = b"IO\0";
    pub const STDOUT_CONST_NAME: &[u8] = b"STDOUT\0";
    pub const STDERR_CONST_NAME: &[u8] = b"STDERR\0";
    pub const STDOUT_GVAR_NAME: &[u8] = b"$stdout\0";
    pub const STDERR_GVAR_NAME: &[u8] = b"$stderr\0";
    pub const MODE_WRITE: &[u8] = b"w\0";
}

// mruby's `nil` / `true` / `false` mrb_values are constructed through
// the shims in `wasm/kobako-wasm/src/mruby/value.c`, which delegate to
// mruby's own `mrb_nil_value()` / `mrb_true_value()` / `mrb_false_value()`
// macros. kobako does NOT mirror the word-box bit pattern in Rust —
// the layout is mruby's business. The Rust side caches the three
// values inside [`Kobako`] at install time so the hot path stays a
// field read rather than a cross-FFI call.

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
    client_class: *mut sys::RClass,
    handle_class: *mut sys::RClass,
    service_error_class: *mut sys::RClass,
    disconnected_class: *mut sys::RClass,
    wire_error_class: *mut sys::RClass,
    /// Cached `mrb_nil_value()` / `mrb_true_value()` / `mrb_false_value()`
    /// snapshots, captured once into the [`Immediates`] static cell at
    /// the first install and read by both [`Kobako::install_raw`] and
    /// [`Kobako::resolve_raw`]. The fields end up identical across
    /// every `Kobako` instance — they live on the struct (instead of
    /// staying as free accessors) because the per-instance read is
    /// hotter than the one-time capture, and field reads keep the
    /// `Kobako::nil_value` / `true_value` / `false_value` accessors
    /// branch-free.
    ///
    /// All three names carry the `q-` prefix even though `nil` is not
    /// a Rust keyword: mruby's source spells the immediates as
    /// `Qnil` / `Qtrue` / `Qfalse`, and keeping all three symmetric in
    /// Rust makes the mruby correspondence obvious.
    qnil: sys::mrb_value,
    qtrue: sys::mrb_value,
    qfalse: sys::mrb_value,
}

/// Process-wide cache for mruby's immediate values
/// (`mrb_nil_value()` / `mrb_true_value()` / `mrb_false_value()`).
///
/// All three are config-level constants under mruby's word-box
/// configuration — they are decided at libmruby build time and do
/// not vary across `mrb_state` instances. Capturing them once via
/// [`Immediates::get`] sidesteps three cross-FFI calls every time a
/// C bridge enters [`Kobako::resolve_raw`].
#[cfg(target_arch = "wasm32")]
struct Immediates {
    qnil: sys::mrb_value,
    qtrue: sys::mrb_value,
    qfalse: sys::mrb_value,
}

// SAFETY: `mrb_value` on wasm32 is a `#[repr(C)] struct { w: u32 }` —
// plain old data with no interior mutability. `Immediates` therefore
// shares only `Copy` snapshots and is trivially Sync across the
// single-threaded wasm execution model.
#[cfg(target_arch = "wasm32")]
unsafe impl Sync for Immediates {}

#[cfg(target_arch = "wasm32")]
static IMMEDIATES: std::sync::OnceLock<Immediates> = std::sync::OnceLock::new();

#[cfg(target_arch = "wasm32")]
impl Immediates {
    /// Return the cached snapshot, capturing it on first call.
    fn get() -> &'static Immediates {
        IMMEDIATES.get_or_init(|| {
            // SAFETY: the three shims read mruby's `mrb_nil_value()` /
            // `mrb_true_value()` / `mrb_false_value()` macros, which
            // do not touch `mrb_state` — see
            // `wasm/kobako-wasm/src/mruby/value.c`.
            unsafe {
                Immediates {
                    qnil: sys::kobako_nil_value(),
                    qtrue: sys::kobako_true_value(),
                    qfalse: sys::kobako_false_value(),
                }
            }
        })
    }
}

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
            // Capture the canonical nil / true / false mrb_values
            // through the process-wide [`Immediates`] cache — the
            // word-box layout is mruby's business, kobako never reads
            // the bit pattern.
            let immediates = Immediates::get();

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
                qnil: immediates.qnil,
                qtrue: immediates.qtrue,
                qfalse: immediates.qfalse,
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
                let client_class = sys::mrb_class_get_under(mrb, rpc_mod, cstr_ptr(CLIENT_NAME));
                let handle_class = sys::mrb_class_get_under(mrb, rpc_mod, cstr_ptr(HANDLE_NAME));
                let service_error_class =
                    sys::mrb_class_get_under(mrb, kobako_mod, cstr_ptr(SERVICE_ERROR_NAME));
                let disconnected_class =
                    sys::mrb_class_get_under(mrb, service_error_class, cstr_ptr(DISCONNECTED_NAME));
                let wire_error_class =
                    sys::mrb_class_get_under(mrb, rpc_mod, cstr_ptr(WIRE_ERROR_NAME));
                let immediates = Immediates::get();
                Self {
                    mrb,
                    client_class,
                    handle_class,
                    service_error_class,
                    disconnected_class,
                    wire_error_class,
                    qnil: immediates.qnil,
                    qtrue: immediates.qtrue,
                    qfalse: immediates.qfalse,
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
                            self.client_class,
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
        sys::mrb_raise(self.mrb, self.wire_error_class, cstr_ptr(msg));
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
        sys::mrb_raise(self.mrb, target_cls, msg.as_ptr());
    }

    // ----------------------------------------------------------------
    // mruby value constructors — `nil` / `true` / `false`.
    //
    // Each returns the cached mrb_value captured at install time by
    // the value.c shim wrappers around `mrb_nil_value()` /
    // `mrb_true_value()` / `mrb_false_value()`. kobako does not know
    // the word-box bit pattern — the layout is mruby's business.
    // ----------------------------------------------------------------

    /// Return the canonical mruby `nil` value. Mirrors mruby's own
    /// `mrb_nil_value()` API; the actual mrb_value was captured at
    /// install time via [`sys::kobako_nil_value`].
    #[cfg(target_arch = "wasm32")]
    pub(crate) fn nil_value(&self) -> sys::mrb_value {
        self.qnil
    }

    /// Return the canonical mruby `true` value. Mirrors mruby's own
    /// `mrb_true_value()` API.
    #[cfg(target_arch = "wasm32")]
    pub(crate) fn true_value(&self) -> sys::mrb_value {
        self.qtrue
    }

    /// Return the canonical mruby `false` value. Mirrors mruby's own
    /// `mrb_false_value()` API.
    #[cfg(target_arch = "wasm32")]
    pub(crate) fn false_value(&self) -> sys::mrb_value {
        self.qfalse
    }

    // ----------------------------------------------------------------
    // Safe FFI primitives.
    //
    // Each method below wraps a single mruby C API call. They are safe
    // by construction under the +Kobako+ contract: holding +&Kobako+
    // proves that +self.mrb+ is live (its only constructor is the
    // wasm32 +install_raw+ arm, called once per +__kobako_eval+ /
    // +__kobako_run+ invocation and only released when the owning
    // +Mrb+ drops), and every +mrb_value+ passed in must originate
    // from that same VM (a single-VM-per-call invariant the guest
    // crate maintains by construction — there is no path that mixes
    // values from two states).
    //
    // The bodies are the only places +unsafe { ... }+ wraps these
    // primitives; callers stop repeating the same SAFETY note at
    // every dispatch site.
    // ----------------------------------------------------------------

    /// Ruby class name of +val+ (or +""+ when mruby returns NULL).
    /// Wraps [`sys::mrb_value::classname`].
    #[cfg(target_arch = "wasm32")]
    pub(crate) fn classname_of(&self, val: sys::mrb_value) -> &'static str {
        // SAFETY: see "Safe FFI primitives" section doc.
        unsafe { val.classname(self.mrb) }
    }

    /// Coerce +val+ to a Rust +String+ via +Object#to_s+. Wraps
    /// [`sys::mrb_value::to_string`].
    #[cfg(target_arch = "wasm32")]
    pub(crate) fn to_string_of(&self, val: sys::mrb_value) -> String {
        // SAFETY: see "Safe FFI primitives" section doc.
        unsafe { val.to_string(self.mrb) }
    }

    /// Invoke +val.method_name(args...)+. +name+ is a NUL-terminated
    /// C string (use the [`cstr!`] macro at call sites). Wraps
    /// [`sys::mrb_value::call`].
    #[cfg(target_arch = "wasm32")]
    pub(crate) fn call_method(
        &self,
        val: sys::mrb_value,
        name: *const core::ffi::c_char,
        args: &[sys::mrb_value],
    ) -> sys::mrb_value {
        // SAFETY: see "Safe FFI primitives" section doc. +name+ must
        // be NUL-terminated; callers obtain it from +cstr!+ or
        // [`cstr_ptr`], both of which guarantee that.
        unsafe { val.call(self.mrb, name, args) }
    }

    /// +mrb_ary_entry(ary, idx)+. No bounds checking — callers must
    /// keep +idx+ within +0..ary.length+.
    #[cfg(target_arch = "wasm32")]
    fn ary_entry(&self, ary: sys::mrb_value, idx: i32) -> sys::mrb_value {
        // SAFETY: see "Safe FFI primitives" section doc.
        unsafe { sys::mrb_ary_entry(ary, idx) }
    }

    /// Direct Integer-tagged predicate via the +kobako_value_is_integer+
    /// C shim (wrapper around mruby's own +mrb_integer_p+ macro).
    /// Cheaper than a +classname_of+ string compare and serves as the
    /// precondition for [`Self::unbox_integer`].
    #[cfg(target_arch = "wasm32")]
    fn value_is_integer(&self, val: sys::mrb_value) -> bool {
        // SAFETY: see "Safe FFI primitives" section doc. The shim does
        // not touch mrb_state.
        (unsafe { sys::kobako_value_is_integer(val) }) != 0
    }

    /// Direct +mrb_integer(v)+ unbox via the +kobako_unbox_integer+ C
    /// shim. Replaces the previous +.to_s.parse+ round-trip; on wasm32
    /// (+MRB_INT32+) the payload is a signed 32-bit integer that fits
    /// in +i64+ without loss. Callers MUST gate on
    /// [`Self::value_is_integer`] — passing a non-Integer value is
    /// undefined behaviour per mruby's macro contract.
    #[cfg(target_arch = "wasm32")]
    fn unbox_integer(&self, val: sys::mrb_value) -> i32 {
        // SAFETY: caller has confirmed +val+ is Integer-tagged via
        // [`Self::value_is_integer`]; the shim does not touch mrb_state.
        unsafe { sys::kobako_unbox_integer(val) }
    }

    /// Direct +mrb_float(v)+ unbox via the +kobako_unbox_float+ C shim.
    /// Preserves full +f64+ precision unlike the previous +.to_s.parse+
    /// path, which went through mruby's +%.16g+ formatter and lost the
    /// last ULP at the edges of representable doubles. Caller must have
    /// confirmed Float-tagging via [`Self::classname_of`] returning
    /// `"Float"` (which is the existing dispatch precondition in
    /// `to_wire_value` / `to_wire_outcome`).
    #[cfg(target_arch = "wasm32")]
    fn unbox_float(&self, val: sys::mrb_value) -> f64 {
        // SAFETY: caller has confirmed +val+ is Float-tagged.
        unsafe { sys::kobako_unbox_float(val) }
    }

    /// +mrb_hash_get(hash, key)+. Returns +nil+ when the key is
    /// missing.
    #[cfg(target_arch = "wasm32")]
    fn hash_get(&self, hash: sys::mrb_value, key: sys::mrb_value) -> sys::mrb_value {
        // SAFETY: see "Safe FFI primitives" section doc.
        unsafe { sys::mrb_hash_get(self.mrb, hash, key) }
    }

    /// +mrb_hash_keys(hash)+ — returns the key Array.
    #[cfg(target_arch = "wasm32")]
    fn hash_keys(&self, hash: sys::mrb_value) -> sys::mrb_value {
        // SAFETY: see "Safe FFI primitives" section doc.
        unsafe { sys::mrb_hash_keys(self.mrb, hash) }
    }

    /// Intern +name+ (NUL-terminated) as an mruby Symbol id.
    #[cfg(target_arch = "wasm32")]
    fn intern(&self, name: *const core::ffi::c_char) -> sys::mrb_sym {
        // SAFETY: see "Safe FFI primitives" section doc.
        unsafe { sys::mrb_intern_cstr(self.mrb, name) }
    }

    /// Set the instance variable +sym+ on +obj+ to +val+.
    #[cfg(target_arch = "wasm32")]
    fn iv_set(&self, obj: sys::mrb_value, sym: sys::mrb_sym, val: sys::mrb_value) {
        // SAFETY: see "Safe FFI primitives" section doc.
        unsafe { sys::mrb_iv_set(self.mrb, obj, sym, val) };
    }

    /// Read the instance variable +sym+ from +obj+ (+nil+ if unset).
    #[cfg(target_arch = "wasm32")]
    fn iv_get(&self, obj: sys::mrb_value, sym: sys::mrb_sym) -> sys::mrb_value {
        // SAFETY: see "Safe FFI primitives" section doc.
        unsafe { sys::mrb_iv_get(self.mrb, obj, sym) }
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
    pub fn collection_len(&self, col: sys::mrb_value) -> usize {
        let len_val = self.call_method(col, cstr!("length"), &[]);
        if !self.value_is_integer(len_val) {
            return 0;
        }
        let len = self.unbox_integer(len_val);
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
    pub fn extract_backtrace(&self, exc_val: sys::mrb_value) -> Vec<String> {
        let bt_val = self.call_method(exc_val, cstr!("backtrace"), &[]);
        if self.classname_of(bt_val) != "Array" {
            return Vec::new();
        }
        let len = self.collection_len(bt_val);
        let mut lines = Vec::with_capacity(len);
        for i in 0..len {
            let line = self.ary_entry(bt_val, i as i32);
            lines.push(self.to_string_of(line));
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
        let object_value = unsafe { sys::kobako_class_value(sys::mrb_object_class(self.mrb)) };
        let consts = self.call_method(object_value, cstr!("constants"), &[]);
        if self.classname_of(consts) != "Array" {
            return Vec::new();
        }
        let len = self.collection_len(consts);
        let mut names = Vec::with_capacity(len);
        for i in 0..len {
            names.push(self.to_string_of(self.ary_entry(consts, i as i32)));
        }
        names
    }

    /// Store `id_val` into a fresh `Kobako::RPC::Handle` instance's
    /// `@__kobako_id__` ivar. Used by the `Kobako::RPC::Handle#initialize`
    /// C bridge.
    #[cfg(target_arch = "wasm32")]
    pub fn set_handle_id(&self, target: sys::mrb_value, id_val: sys::mrb_value) {
        let sym = self.intern(cstr_ptr(HANDLE_ID_IVAR));
        self.iv_set(target, sym, id_val);
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
    pub fn extract_handle_id(&self, handle_val: sys::mrb_value) -> u32 {
        let id_sym = self.intern(cstr_ptr(HANDLE_ID_IVAR));
        let id_val = self.iv_get(handle_val, id_sym);
        if !self.value_is_integer(id_val) {
            return 0;
        }
        let id = self.unbox_integer(id_val);
        if id < 0 {
            0
        } else {
            id as u32
        }
    }

    // ----------------------------------------------------------------
    // RPC dispatch.
    // ----------------------------------------------------------------

    /// Invoke `invoke_rpc` and convert the result to an `mrb_value`. On
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
    ) -> sys::mrb_value {
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
