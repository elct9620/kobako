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
//! [`Kobako::install`] is called once per `__kobako_run` invocation,
//! immediately after [`Mrb::open`]. It registers every boot-time entity
//! and returns a `Kobako` carrying the resolved class handles. The
//! returned value is then used to drive the Frame 1 preamble through
//! [`Kobako::install_groups`].
//!
//! C-bridges that receive a raw `*mut mrb_state` from mruby use the
//! [`Kobako::resolve_raw`] entry to obtain the same handle without
//! repeating registration.

#[cfg(any(target_arch = "wasm32", test))]
pub mod bridges;
pub mod bytecode;
#[cfg(any(target_arch = "wasm32", test))]
pub mod io;

#[cfg(target_arch = "wasm32")]
use crate::cstr;
use crate::mruby::sys;
#[cfg(target_arch = "wasm32")]
use crate::mruby::value::cstr_ptr;
use crate::mruby::Mrb;
#[cfg(target_arch = "wasm32")]
use crate::rpc::client::ExceptionPayload;

// --------------------------------------------------------------------
// C-string constants — NUL-terminated names passed to the mruby C API.
// --------------------------------------------------------------------

#[cfg(target_arch = "wasm32")]
const KOBAKO_NAME: &[u8] = b"Kobako\0";
#[cfg(target_arch = "wasm32")]
const RPC_NAME: &[u8] = b"RPC\0";
#[cfg(target_arch = "wasm32")]
const CLIENT_NAME: &[u8] = b"Client\0";
#[cfg(target_arch = "wasm32")]
const HANDLE_NAME: &[u8] = b"Handle\0";
#[cfg(target_arch = "wasm32")]
const METHOD_MISSING_NAME: &[u8] = b"method_missing\0";
#[cfg(target_arch = "wasm32")]
const RESPOND_TO_MISSING_NAME: &[u8] = b"respond_to_missing?\0";
#[cfg(target_arch = "wasm32")]
const INITIALIZE_NAME: &[u8] = b"initialize\0";
#[cfg(target_arch = "wasm32")]
const SERVICE_ERROR_NAME: &[u8] = b"ServiceError\0";
#[cfg(target_arch = "wasm32")]
const DISCONNECTED_NAME: &[u8] = b"Disconnected\0";
#[cfg(target_arch = "wasm32")]
const RUNTIME_ERROR_NAME: &[u8] = b"RuntimeError\0";
#[cfg(target_arch = "wasm32")]
const WIRE_ERROR_NAME: &[u8] = b"WireError\0";
/// `b"@__kobako_id__\0"` — mangled instance-variable name that
/// `Kobako::RPC::Handle#initialize` stores the Handle id under. Used by the
/// handle-id setter / getter on [`Kobako`].
#[cfg(target_arch = "wasm32")]
const HANDLE_ID_IVAR: &[u8] = b"@__kobako_id__\0";
#[cfg(target_arch = "wasm32")]
const IO_NAME: &[u8] = b"IO\0";
#[cfg(target_arch = "wasm32")]
const STDOUT_CONST_NAME: &[u8] = b"STDOUT\0";
#[cfg(target_arch = "wasm32")]
const STDERR_CONST_NAME: &[u8] = b"STDERR\0";
#[cfg(target_arch = "wasm32")]
const STDOUT_GVAR_NAME: &[u8] = b"$stdout\0";
#[cfg(target_arch = "wasm32")]
const STDERR_GVAR_NAME: &[u8] = b"$stderr\0";
#[cfg(target_arch = "wasm32")]
const MODE_WRITE: &[u8] = b"w\0";

// mruby word-boxing constants for MRB_WORDBOX_NO_INLINE_FLOAT + MRB_INT32
// (the wasm32 build config). Bit-pattern values from mruby.h; must not
// change without verifying the mruby header for the targeted version.
const MRB_QNIL: u32 = 0; // MRB_Qnil
#[cfg(target_arch = "wasm32")]
const MRB_QTRUE: u32 = 12; // MRB_Qtrue
#[cfg(target_arch = "wasm32")]
const MRB_QFALSE: u32 = 4; // MRB_Qfalse
                           // MRB_Qnil must be zero so `mrb_value::zeroed()` produces a nil value.
const _: () = assert!(MRB_QNIL == 0, "MRB_Qnil must be zero (zeroed() == nil)");

/// Failures returned by [`Kobako::install_groups`] when a preamble entry
/// carries a name that cannot be passed through the mruby C API (which
/// expects NUL-terminated strings).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallGroupsError {
    /// A Group name contained an interior NUL byte.
    NulInGroupName,
    /// A Member name contained an interior NUL byte.
    NulInMemberName,
}

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

impl std::error::Error for InstallGroupsError {}

/// Handle to a Kobako runtime installed on a live mruby VM.
///
/// `Kobako` is a value-type token: it carries the raw `*mut mrb_state`
/// pointer plus the resolved class handles, but does not own the VM —
/// the caller is responsible for keeping the underlying state live for
/// the duration of any `Kobako` method call. Constructed through one of
/// the four entry points:
///
///   * [`Kobako::install`] / [`Kobako::install_raw`] — register every
///     boot-time entity then return a fully populated handle.
///   * [`Kobako::resolve`] / [`Kobako::resolve_raw`] — re-resolve class
///     handles produced by a prior install (used by C-bridges).
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
}

/// Host-target stub for [`Kobako`]. See the wasm32 declaration above
/// for the production-target shape and field-level docs.
#[cfg(not(target_arch = "wasm32"))]
pub struct Kobako {}

impl Kobako {
    /// Install the Kobako runtime onto `mrb` and return a handle to the
    /// resulting class registrations. Safe wrapper over
    /// [`Kobako::install_raw`].
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
            // SAFETY of every FFI call in this block:
            //
            //   * `mrb` is live by the function's safety contract.
            //   * Every C-string passed (`cstr_ptr(*_NAME)`) is a
            //     compile-time-NUL-terminated `&[u8]`, so the pointer
            //     conversion satisfies mruby's `const char*` requirement.
            //   * Class handles returned by `mrb_define_module` /
            //     `mrb_define_class_under` / `mrb_class_get` /
            //     `mrb_module_get` are owned by mruby and live for the
            //     duration of `mrb`; we use them only inside this
            //     function body and stash the load-bearing five in
            //     `Self`, which itself lives no longer than `mrb`.
            //   * The function-pointer arguments are
            //     `unsafe extern "C" fn` items from
            //     [`bridges`], the only producer of the
            //     `mrb_func_t` signature in this crate.
            unsafe {
                // (1) Kobako module.
                let kobako_mod = sys::mrb_define_module(mrb, cstr_ptr(KOBAKO_NAME));

                // (2) Kobako::RPC module — protocol namespace shared
                // with the host gem's lib/kobako/rpc.rb. Houses the
                // Client base class plus Handle / WireError value
                // objects that ride on the wire.
                let rpc_mod = sys::mrb_define_module_under(mrb, kobako_mod, cstr_ptr(RPC_NAME));

                // (3) Kobako::RPC::Client base class — parent of every
                // Member installed via `Kobako::install_groups`. Spell the
                // super class as `(*mrb).object_class` to match the
                // mrbgems/mruby-io convention (see
                // `crate::mruby::sys::mrb_state` doc). `_under` would
                // silently fall back to Object on NULL, but the kobako
                // convention is "never hand mruby a NULL super" — uniform
                // with the top-level `IO` install in `crate::kobako::io`.
                let client_class = sys::mrb_define_class_under(
                    mrb,
                    rpc_mod,
                    cstr_ptr(CLIENT_NAME),
                    (*mrb).object_class,
                );

                // (4) Singleton-class `method_missing` /
                //     `respond_to_missing?` on `Kobako::RPC::Client`.
                //     Subclasses inherit through metaclass-chain
                //     dispatch.
                sys::mrb_define_singleton_method(
                    mrb,
                    client_class as *mut sys::RObject,
                    cstr_ptr(METHOD_MISSING_NAME),
                    bridges::rpc_method_missing,
                    sys::MRB_ARGS_ANY,
                );
                sys::mrb_define_singleton_method(
                    mrb,
                    client_class as *mut sys::RObject,
                    cstr_ptr(RESPOND_TO_MISSING_NAME),
                    bridges::rpc_respond_to_missing,
                    sys::MRB_ARGS_ANY,
                );

                // (5) `Kobako::RPC::Handle` instance class. Same explicit
                // `(*mrb).object_class` super as the Client class above.
                let handle_class = sys::mrb_define_class_under(
                    mrb,
                    rpc_mod,
                    cstr_ptr(HANDLE_NAME),
                    (*mrb).object_class,
                );
                sys::mrb_define_method(
                    mrb,
                    handle_class,
                    cstr_ptr(INITIALIZE_NAME),
                    bridges::handle_initialize,
                    sys::mrb_args_req(1),
                );
                sys::mrb_define_method(
                    mrb,
                    handle_class,
                    cstr_ptr(METHOD_MISSING_NAME),
                    bridges::handle_method_missing,
                    sys::MRB_ARGS_ANY,
                );
                sys::mrb_define_method(
                    mrb,
                    handle_class,
                    cstr_ptr(RESPOND_TO_MISSING_NAME),
                    bridges::rpc_respond_to_missing,
                    sys::MRB_ARGS_ANY,
                );

                // (6) `Kobako::ServiceError` /
                //     `Kobako::ServiceError::Disconnected` /
                //     `Kobako::RPC::WireError` — all subclass
                //     `RuntimeError`. ServiceError stays at the Kobako
                //     top level (L104 public API); WireError lives
                //     under RPC since it is an RPC-layer fault.
                let runtime_error_class = sys::mrb_class_get(mrb, cstr_ptr(RUNTIME_ERROR_NAME));
                let service_error_class = sys::mrb_define_class_under(
                    mrb,
                    kobako_mod,
                    cstr_ptr(SERVICE_ERROR_NAME),
                    runtime_error_class,
                );
                let disconnected_class = sys::mrb_define_class_under(
                    mrb,
                    service_error_class,
                    cstr_ptr(DISCONNECTED_NAME),
                    service_error_class,
                );
                let wire_error_class = sys::mrb_define_class_under(
                    mrb,
                    rpc_mod,
                    cstr_ptr(WIRE_ERROR_NAME),
                    runtime_error_class,
                );

                // (7) Top-level `::IO` class. Registers the constructor
                //     + `#write` / `#fileno` C bridges and then loads
                //     `mrblib/io.rb` to layer the rest of the IO surface
                //     (`#print`, `#puts`, `#printf`, `#p`, `#<<`, etc.)
                //     in pure Ruby. The bridges talk to wasi-libc's
                //     `stdout` / `stderr` via the `kobako_io_fwrite` C
                //     shim, so guest output reaches the host capture
                //     pipe (SPEC.md B-04) without re-entering the RPC
                //     dispatch path. See `crate::kobako::io`.
                io::install(mrb);

                // (8) Construct `STDOUT` / `STDERR` and wire `$stdout` /
                //     `$stderr` to them. Both globals are reassignable
                //     by guest scripts (`$stdout = $stderr` redirects
                //     subsequent `Kernel#puts` output to stderr), which
                //     is the whole point of routing through the
                //     mrblib/kernel.rb delegators below.
                let io_class = sys::mrb_class_get(mrb, cstr_ptr(IO_NAME));
                let mode_str = sys::mrb_str_new_cstr(mrb, cstr_ptr(MODE_WRITE));
                let stdout_args = [sys::mrb_boxing_int_value(mrb, 1), mode_str];
                let stdout_val = sys::mrb_obj_new(mrb, io_class, 2, stdout_args.as_ptr());
                let stderr_args = [sys::mrb_boxing_int_value(mrb, 2), mode_str];
                let stderr_val = sys::mrb_obj_new(mrb, io_class, 2, stderr_args.as_ptr());

                sys::mrb_define_global_const(mrb, cstr_ptr(STDOUT_CONST_NAME), stdout_val);
                sys::mrb_define_global_const(mrb, cstr_ptr(STDERR_CONST_NAME), stderr_val);

                let stdout_gvar = sys::mrb_intern_cstr(mrb, cstr_ptr(STDOUT_GVAR_NAME));
                let stderr_gvar = sys::mrb_intern_cstr(mrb, cstr_ptr(STDERR_GVAR_NAME));
                sys::mrb_gv_set(mrb, stdout_gvar, stdout_val);
                sys::mrb_gv_set(mrb, stderr_gvar, stderr_val);

                // (9) Kernel output delegators. `mrblib/kernel.rb`
                //     redefines `Kernel#print` (overriding the
                //     mruby-core `mrb_print_m` registration that always
                //     targets the C `stdout` FILE*) and adds `#puts`,
                //     `#p`, `#printf`, `#warn` as thin pass-throughs to
                //     `$stdout` / `$stderr`. Must run after (8) — the
                //     delegators look up the globals at call time but
                //     would NoMethodError if called before they exist.
                bytecode::load(mrb, bytecode::KERNEL_MRB);

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
        #[cfg(not(target_arch = "wasm32"))]
        {
            Self {}
        }
    }

    /// Resolve the class handles produced by a prior install. Safe
    /// wrapper over [`Kobako::resolve_raw`].
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    pub fn resolve(mrb: &Mrb) -> Self {
        // SAFETY: `mrb` is a live, non-closed state per the `&Mrb`
        // borrow.
        unsafe { Self::resolve_raw(mrb.as_ptr()) }
    }

    /// Resolve the class handles produced by a prior install, from a
    /// raw `*mut mrb_state`.
    ///
    /// # Safety
    ///
    /// `mrb` must be a live mruby state on which [`Kobako::install`] /
    /// [`Kobako::install_raw`] has already run. Calling this against a
    /// fresh state without prior install would surface as a NULL
    /// `mrb_class_get_under` return value and later UB; the C-bridge
    /// entry points uphold the install precondition by construction
    /// (they are invoked through registrations done at install time).
    #[cfg_attr(not(target_arch = "wasm32"), allow(unused_variables))]
    pub unsafe fn resolve_raw(mrb: *mut sys::mrb_state) -> Self {
        #[cfg(target_arch = "wasm32")]
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
        #[cfg(not(target_arch = "wasm32"))]
        {
            Self {}
        }
    }

    /// Install Namespace / Member proxy classes from a Frame 1
    /// preamble. Each Group becomes a top-level Ruby module; each Member
    /// becomes a subclass of `Kobako::RPC::Client` under its Namespace so the
    /// singleton-class `method_missing` shim is inherited.
    pub fn install_groups(
        &self,
        preamble: &[(String, Vec<String>)],
    ) -> Result<(), InstallGroupsError> {
        #[cfg(target_arch = "wasm32")]
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
        #[cfg(not(target_arch = "wasm32"))]
        {
            let _ = preamble;
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
        sys::mrb_raise(
            self.mrb,
            self.wire_error_class,
            msg.as_ptr() as *const core::ffi::c_char,
        );
    }

    /// Raise the matching `Kobako::ServiceError` subclass for `ex`.
    /// Diverges — `mrb_raise` does not return.
    ///
    /// SPEC.md "Error Classes" + "Error Envelope" pin the mapping from
    /// the Response.err `type` field to a guest-side mruby class. Only
    /// `"disconnected"` resolves to a named subclass today
    /// (`Kobako::ServiceError::Disconnected`); the other three reserved
    /// types and any future unmapped type land on the parent
    /// `Kobako::ServiceError`.
    ///
    /// # Safety
    ///
    /// As [`Kobako::raise_wire_error`].
    #[cfg(target_arch = "wasm32")]
    pub unsafe fn raise_service_error(&self, ex: &ExceptionPayload) -> ! {
        let target_cls = if ex.kind == "disconnected" {
            self.disconnected_class
        } else {
            self.service_error_class
        };
        let msg = std::ffi::CString::new(ex.message.as_str()).unwrap_or_default();
        sys::mrb_raise(self.mrb, target_cls, msg.as_ptr());
    }

    /// Raw mruby state pointer. Reserved for FFI sites that still must
    /// talk to the C API directly (`mrb_get_args`, `mrb_ary_entry`, …);
    /// every helper expressible as a method on [`Kobako`] should prefer
    /// that surface.
    #[cfg(target_arch = "wasm32")]
    pub fn as_ptr(&self) -> *mut sys::mrb_state {
        self.mrb
    }

    // ----------------------------------------------------------------
    // mruby value constructors — `nil` / `true` / `false`.
    // ----------------------------------------------------------------

    /// Return an mruby `nil` value. `MRB_Qnil == 0`, so this is the
    /// same as `mrb_value::zeroed()` on wasm32; the named accessor
    /// keeps call sites explicit about intent and mirrors mruby's own
    /// `mrb_nil_value()` API.
    #[cfg(target_arch = "wasm32")]
    pub fn nil_value(&self) -> sys::mrb_value {
        sys::mrb_value { w: MRB_QNIL }
    }

    /// Return an mruby `true` value (`MRB_Qtrue == 12`). Mirrors
    /// mruby's own `mrb_true_value()` API.
    #[cfg(target_arch = "wasm32")]
    pub fn true_value(&self) -> sys::mrb_value {
        sys::mrb_value { w: MRB_QTRUE }
    }

    /// Return an mruby `false` value (`MRB_Qfalse == 4`). Mirrors
    /// mruby's own `mrb_false_value()` API.
    #[cfg(target_arch = "wasm32")]
    pub fn false_value(&self) -> sys::mrb_value {
        sys::mrb_value { w: MRB_QFALSE }
    }

    // ----------------------------------------------------------------
    // Collection / hash / handle helpers.
    // ----------------------------------------------------------------

    /// Return the number of elements in an mruby Array or Hash by
    /// calling `.length` and parsing the result string. Used wherever a
    /// collection length is needed without a direct FFI binding for
    /// `mrb_ary_len`.
    #[cfg(target_arch = "wasm32")]
    pub fn collection_len(&self, col: sys::mrb_value) -> usize {
        // SAFETY: `self.mrb` is live (Kobako construction precondition);
        // `col` is an mrb_value produced by the same VM.
        let len_val = unsafe { col.call(self.mrb, cstr!("length"), &[]) };
        unsafe { len_val.to_string(self.mrb) }.parse().unwrap_or(0)
    }

    /// Collect `exc_val.backtrace` (an mruby `Array of String`) into a
    /// Rust `Vec<String>`. Used by the guest panic path
    /// (`crate::abi::__kobako_run`) to populate the Panic envelope's
    /// `backtrace` field per SPEC.md "Panic Envelope" L876.
    ///
    /// mruby's default build keeps the backtrace, so `.backtrace`
    /// returns an Array of String. If the runtime is ever rebuilt
    /// without keep-mode the call yields a non-Array value (typically
    /// `nil`); fall back to an empty vec so the Panic envelope still
    /// serializes cleanly.
    #[cfg(target_arch = "wasm32")]
    pub fn extract_backtrace(&self, exc_val: sys::mrb_value) -> Vec<String> {
        // SAFETY: `self.mrb` is live; `exc_val` is an mrb_value produced
        // by the same VM.
        unsafe {
            let bt_val = exc_val.call(self.mrb, cstr!("backtrace"), &[]);
            if bt_val.classname(self.mrb) != "Array" {
                return Vec::new();
            }
            let len = self.collection_len(bt_val);
            let mut lines = Vec::with_capacity(len);
            for i in 0..len {
                let line = sys::mrb_ary_entry(bt_val, i as i32);
                lines.push(line.to_string(self.mrb));
            }
            lines
        }
    }

    /// Store `id_val` into a fresh `Kobako::RPC::Handle` instance's
    /// `@__kobako_id__` ivar. Used by the `Kobako::RPC::Handle#initialize`
    /// C bridge.
    #[cfg(target_arch = "wasm32")]
    pub fn set_handle_id(&self, target: sys::mrb_value, id_val: sys::mrb_value) {
        // SAFETY: bridge frame — both values are mrb_values from the
        // same VM.
        unsafe {
            let sym = sys::mrb_intern_cstr(self.mrb, cstr_ptr(HANDLE_ID_IVAR));
            sys::mrb_iv_set(self.mrb, target, sym, id_val);
        }
    }

    /// Read the `u32` Handle id stored in a `Kobako::RPC::Handle` instance's
    /// `@__kobako_id__` instance variable. Returns 0 when the ivar is
    /// missing or non-numeric — the resolver downstream treats id 0 as
    /// undefined per SPEC.md B-19.
    #[cfg(target_arch = "wasm32")]
    pub fn extract_handle_id(&self, handle_val: sys::mrb_value) -> u32 {
        // SAFETY: as above.
        unsafe {
            let id_sym = sys::mrb_intern_cstr(self.mrb, cstr_ptr(HANDLE_ID_IVAR));
            let id_val = sys::mrb_iv_get(self.mrb, handle_val, id_sym);
            id_val.to_string(self.mrb).parse().unwrap_or(0)
        }
    }

    /// Decode every key/value pair from an mruby Hash into `out` as
    /// `(String, codec::Value)` pairs. The outer `String` carries the
    /// key's name; [`crate::rpc::envelope::encode_request`] re-emits each name
    /// as a wire-level `Value::Sym` (ext 0x00) per SPEC.md → Wire Codec
    /// → Ext Types. Keys arriving as either mruby `Symbol` or `String`
    /// reduce to the same UTF-8 name via `Object#to_s`. Values go
    /// through [`Kobako::mrb_value_to_wire_value`].
    #[cfg(target_arch = "wasm32")]
    pub fn decode_hash_kwargs(
        &self,
        hash: sys::mrb_value,
        out: &mut Vec<(String, crate::codec::Value)>,
    ) {
        // SAFETY: `self.mrb` is live; `hash` is an mrb_value produced by
        // the same VM.
        let keys_ary = unsafe { sys::mrb_hash_keys(self.mrb, hash) };
        let keys_len = self.collection_len(keys_ary);
        for i in 0..keys_len {
            let key_val = unsafe { sys::mrb_ary_entry(keys_ary, i as i32) };
            let val = unsafe { sys::mrb_hash_get(self.mrb, hash, key_val) };
            out.push((
                unsafe { key_val.to_string(self.mrb) },
                self.mrb_value_to_wire_value(val),
            ));
        }
    }

    /// Split a `rest` slice (from `mrb_get_args` `"n*"`) into positional
    /// wire args and keyword wire kwargs. The last element is absorbed
    /// into kwargs when it is a Hash; all other elements become
    /// positional args.
    #[cfg(target_arch = "wasm32")]
    pub fn unpack_args_kwargs(
        &self,
        rest: &[sys::mrb_value],
    ) -> (Vec<crate::codec::Value>, Vec<(String, crate::codec::Value)>) {
        let mut wire_args: Vec<crate::codec::Value> = Vec::new();
        let mut wire_kwargs: Vec<(String, crate::codec::Value)> = Vec::new();

        for (idx, &mrb_val) in rest.iter().enumerate() {
            // SAFETY: `self.mrb` is live; `mrb_val` from the same VM.
            let is_hash = unsafe { mrb_val.classname(self.mrb) == "Hash" } && idx == rest.len() - 1;
            if is_hash {
                self.decode_hash_kwargs(mrb_val, &mut wire_kwargs);
            } else {
                wire_args.push(self.mrb_value_to_wire_value(mrb_val));
            }
        }

        (wire_args, wire_kwargs)
    }

    // ----------------------------------------------------------------
    // Wire ↔ mrb_value conversion.
    // ----------------------------------------------------------------

    /// Iterate an mruby Array and convert each element via `convert`,
    /// returning a `Vec<Value>` ready to wrap in [`Value::Array`].
    /// `convert` is a function pointer so the two consumer converters
    /// ([`Kobako::mrb_value_to_wire_value`] and
    /// [`Kobako::mrb_value_to_wire_outcome`]) can share the iteration
    /// while preserving their per-converter recursion target — the
    /// outcome path must keep recursing on `mrb_value_to_wire_outcome`
    /// so unknown nested types fall back to `inspect`, not `to_s`.
    #[cfg(target_arch = "wasm32")]
    fn array_to_wire(
        &self,
        val: sys::mrb_value,
        convert: fn(&Self, sys::mrb_value) -> crate::codec::Value,
    ) -> Vec<crate::codec::Value> {
        let len = self.collection_len(val);
        let mut items = Vec::with_capacity(len);
        for i in 0..len {
            // SAFETY: `val` is an Array mrb_value from `self.mrb`; index is in range.
            let elem = unsafe { sys::mrb_ary_entry(val, i as i32) };
            items.push(convert(self, elem));
        }
        items
    }

    /// Iterate an mruby Hash and convert each key/value pair via
    /// `convert`, returning a `Vec<(Value, Value)>` ready to wrap in
    /// [`Value::Map`]. Both the key and the value flow through the
    /// same `convert` so a `Symbol` key arrives as [`Value::Sym`]
    /// (ext 0x00) and a `String` key as [`Value::Str`] — distinct on
    /// the wire per SPEC.md Ext Types.
    #[cfg(target_arch = "wasm32")]
    fn hash_to_wire(
        &self,
        val: sys::mrb_value,
        convert: fn(&Self, sys::mrb_value) -> crate::codec::Value,
    ) -> Vec<(crate::codec::Value, crate::codec::Value)> {
        // SAFETY: `val` is a Hash mrb_value from `self.mrb`.
        let keys_ary = unsafe { sys::mrb_hash_keys(self.mrb, val) };
        let len = self.collection_len(keys_ary);
        let mut pairs = Vec::with_capacity(len);
        for i in 0..len {
            let key = unsafe { sys::mrb_ary_entry(keys_ary, i as i32) };
            let v = unsafe { sys::mrb_hash_get(self.mrb, val, key) };
            pairs.push((convert(self, key), convert(self, v)));
        }
        pairs
    }

    /// Convert an `mrb_value` to a kobako wire [`crate::codec::Value`]
    /// for use as an RPC argument or keyword value. Symbol values map to
    /// [`Value::Sym`] (ext 0x00, SPEC.md → Wire Codec → Ext Types).
    /// Array / Hash values map to [`Value::Array`] / [`Value::Map`]
    /// recursively (SPEC.md Type Mapping #7-#8). Unknown types fall
    /// back to `Object#to_s`.
    ///
    /// ## Why two converters
    ///
    /// This is the **RPC-path** converter. Hash arguments are still
    /// decoded into kwargs separately via [`Kobako::decode_hash_kwargs`]
    /// when they trail the positional list; a Hash that arrives here is
    /// either nested inside an Array argument or sitting in a non-final
    /// positional slot, and travels natively as [`Value::Map`]. The
    /// sibling [`Kobako::mrb_value_to_wire_outcome`] handles the
    /// **outcome-path** (the script's last-expression value) and uses
    /// `inspect` for its unknown-type fallback instead. Do not unify
    /// the two: the outcome path is read as a display representation,
    /// while RPC arguments are interchange values that reach a
    /// Service's `public_send`.
    #[cfg(target_arch = "wasm32")]
    pub fn mrb_value_to_wire_value(&self, val: sys::mrb_value) -> crate::codec::Value {
        use crate::codec::Value;
        // SAFETY: `self.mrb` is live; `val` from the same VM.
        let classname = unsafe { val.classname(self.mrb) };
        match classname {
            "NilClass" => Value::Nil,
            "TrueClass" => Value::Bool(true),
            "FalseClass" => Value::Bool(false),
            "Integer" => Value::Int(unsafe { val.to_string(self.mrb) }.parse().unwrap_or(0)),
            "Float" => Value::Float(unsafe { val.to_string(self.mrb) }.parse().unwrap_or(0.0)),
            "String" => Value::Str(unsafe { val.to_string(self.mrb) }),
            "Symbol" => Value::Sym(unsafe { val.to_string(self.mrb) }),
            "Array" => Value::Array(self.array_to_wire(val, Self::mrb_value_to_wire_value)),
            "Hash" => Value::Map(self.hash_to_wire(val, Self::mrb_value_to_wire_value)),
            // Fallback: route through `.to_s`.
            _ => Value::Str(unsafe { val.to_string(self.mrb) }),
        }
    }

    /// Convert an `mrb_value` to a kobako wire [`crate::codec::Value`]
    /// for inclusion in the outcome Result envelope. Used by
    /// `__kobako_run` to serialize the user script's last-expression
    /// value. Array / Hash values map to [`Value::Array`] /
    /// [`Value::Map`] recursively (SPEC.md Type Mapping #7-#8) so a
    /// script returning a collection retains element-level fidelity.
    ///
    /// ## Why this differs from [`Kobako::mrb_value_to_wire_value`]
    ///
    /// Unknown types fall back to `Object#inspect` rather than
    /// `Object#to_s`. The outcome envelope is read by host-side callers
    /// as a *display* representation, not an interchange value, so
    /// `inspect` (which quotes strings, shows class names) is the right
    /// shape. Nested values inside an Array or Hash also flow through
    /// `inspect` for unknown types — the recursive call lands back in
    /// this same arm.
    #[cfg(target_arch = "wasm32")]
    pub fn mrb_value_to_wire_outcome(&self, val: sys::mrb_value) -> crate::codec::Value {
        use crate::codec::Value;
        // SAFETY: as above.
        let classname = unsafe { val.classname(self.mrb) };
        match classname {
            "NilClass" => Value::Nil,
            "TrueClass" => Value::Bool(true),
            "FalseClass" => Value::Bool(false),
            "Integer" => Value::Int(unsafe { val.to_string(self.mrb) }.parse().unwrap_or(0)),
            "Float" => Value::Float(unsafe { val.to_string(self.mrb) }.parse().unwrap_or(0.0)),
            "String" => Value::Str(unsafe { val.to_string(self.mrb) }),
            "Symbol" => Value::Sym(unsafe { val.to_string(self.mrb) }),
            "Array" => Value::Array(self.array_to_wire(val, Self::mrb_value_to_wire_outcome)),
            "Hash" => Value::Map(self.hash_to_wire(val, Self::mrb_value_to_wire_outcome)),
            _ => Value::Str(unsafe {
                val.call(self.mrb, cstr!("inspect"), &[])
                    .to_string(self.mrb)
            }),
        }
    }

    /// Convert a kobako wire [`crate::codec::Value`] into an `mrb_value`
    /// suitable for handing back to the mruby VM. Handle values are
    /// boxed into a fresh `Kobako::RPC::Handle` instance carrying the id
    /// (subsequent method calls on it route to the host through
    /// `Kobako::RPC::Handle#method_missing` → [`Self::dispatch_invoke`],
    /// SPEC.md B-17).
    #[cfg(target_arch = "wasm32")]
    pub fn wire_value_to_mrb(&self, val: crate::codec::Value) -> sys::mrb_value {
        use crate::codec::Value;
        // SAFETY: `self.mrb` is live; cached class refs were produced by
        // `install_raw` / `resolve_raw`.
        unsafe {
            match val {
                Value::Nil => self.nil_value(),
                Value::Bool(b) => {
                    if b {
                        self.true_value()
                    } else {
                        self.false_value()
                    }
                }
                Value::Int(n) => {
                    // mrb_int on wasm32 is 32-bit (MRB_INT32); clamp to i32.
                    let n32 = n.clamp(i32::MIN as i64, i32::MAX as i64) as i32;
                    sys::mrb_boxing_int_value(self.mrb, n32)
                }
                Value::UInt(n) => {
                    let n32 = n.min(i32::MAX as u64) as i32;
                    sys::mrb_boxing_int_value(self.mrb, n32)
                }
                Value::Float(f) => sys::mrb_word_boxing_float_value(self.mrb, f),
                Value::Str(s) => match std::ffi::CString::new(s.as_str()) {
                    Ok(cs) => sys::mrb_str_new_cstr(self.mrb, cs.as_ptr()),
                    Err(_) => sys::mrb_str_new(
                        self.mrb,
                        s.as_ptr() as *const core::ffi::c_char,
                        s.len() as i32,
                    ),
                },
                Value::Handle(id) => {
                    let id_val = sys::mrb_boxing_int_value(self.mrb, id as i32);
                    sys::mrb_obj_new(
                        self.mrb,
                        self.handle_class,
                        1,
                        &id_val as *const sys::mrb_value,
                    )
                }
                Value::Bin(bytes) => sys::mrb_str_new(
                    self.mrb,
                    bytes.as_ptr() as *const core::ffi::c_char,
                    bytes.len() as i32,
                ),
                Value::Sym(name) => {
                    // Intern via String#to_sym — mruby's mrb_symbol_value
                    // bit-layout is build-private (we use
                    // MRB_WORDBOX_NO_INLINE_FLOAT) so we go through the VM.
                    let str_val = sys::mrb_str_new(
                        self.mrb,
                        name.as_ptr() as *const core::ffi::c_char,
                        name.len() as i32,
                    );
                    str_val.call(self.mrb, cstr!("to_sym"), &[])
                }
                Value::Array(items) => {
                    let ary = sys::mrb_ary_new(self.mrb);
                    for item in items {
                        let elem = self.wire_value_to_mrb(item);
                        sys::mrb_ary_push(self.mrb, ary, elem);
                    }
                    ary
                }
                Value::Map(pairs) => {
                    let hash = sys::mrb_hash_new(self.mrb);
                    for (k, v) in pairs {
                        let key = self.wire_value_to_mrb(k);
                        let val = self.wire_value_to_mrb(v);
                        sys::mrb_hash_set(self.mrb, hash, key, val);
                    }
                    hash
                }
                // ext 0x02 envelopes are consumed by the exception path
                // (`raise_service_error`) before reaching value
                // conversion; the defensive nil here covers any
                // malformed Response that smuggles one through.
                Value::ErrEnv(_) => self.nil_value(),
            }
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
            Ok(wire_val) => self.wire_value_to_mrb(wire_val),
            Err(crate::rpc::client::InvokeError::ServiceErr(ex)) => {
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
}
