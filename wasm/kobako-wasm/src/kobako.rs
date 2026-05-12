//! Kobako runtime — installs the Kobako module surface onto an mruby VM
//! and owns the class handles needed by the dispatch layer.
//!
//! ## Why a separate type from [`crate::mruby::Mrb`]
//!
//! `Mrb` is the language-level VM owner: it knows how to open and close
//! an mruby state and nothing about kobako's own object surface. The
//! kobako-specific registrations (`Kobako` module, `Kobako::RPC` base
//! class, `Kobako::Handle`, `Kobako::ServiceError` /
//! `Kobako::WireError`, `Kernel#puts` / `Kernel#p` shims) belong to a
//! different concern and live behind this domain boundary.
//!
//! The shape mirrors `magnus::Ruby` for CRuby: a value-type "token" that
//! proves you can talk to the runtime, with no Drop and no lifetime —
//! liveness is the caller's contract, just as it is for mruby's own C
//! API. The C-bridges in [`crate::boot`] remain `unsafe extern "C" fn`
//! callbacks invoked by mruby, but their bodies acquire a [`Kobako`]
//! through [`Kobako::resolve_raw`] and then call safe methods.
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

pub mod bridges;

#[cfg(target_arch = "wasm32")]
use crate::cstr;
use crate::mruby::sys;
#[cfg(target_arch = "wasm32")]
use crate::mruby::value::cstr_ptr;
use crate::mruby::Mrb;
#[cfg(target_arch = "wasm32")]
use crate::rpc_client::ExceptionPayload;

// --------------------------------------------------------------------
// C-string constants — NUL-terminated names passed to the mruby C API.
// --------------------------------------------------------------------

const KOBAKO_NAME: &[u8] = b"Kobako\0";
const RPC_NAME: &[u8] = b"RPC\0";
#[cfg(target_arch = "wasm32")]
const HANDLE_NAME: &[u8] = b"Handle\0";
#[cfg(target_arch = "wasm32")]
const RPC_CALL_NAME: &[u8] = b"__rpc_call__\0";
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
#[cfg(target_arch = "wasm32")]
const KERNEL_NAME: &[u8] = b"Kernel\0";
#[cfg(target_arch = "wasm32")]
const PUTS_NAME: &[u8] = b"puts\0";
#[cfg(target_arch = "wasm32")]
const P_NAME: &[u8] = b"p\0";
/// `b"print\0"` — `Kernel#print`, the mrbgem-provided method that the
/// `Kernel#puts` / `Kernel#p` shims delegate to. Used by
/// [`Kobako::print_str`].
#[cfg(target_arch = "wasm32")]
const PRINT_NAME: &[u8] = b"print\0";
/// `b"@__kobako_id__\0"` — mangled instance-variable name that
/// `Kobako::Handle#initialize` stores the Handle id under. Used by the
/// handle-id setter / getter on [`Kobako`].
#[cfg(target_arch = "wasm32")]
const HANDLE_ID_IVAR: &[u8] = b"@__kobako_id__\0";

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
pub struct Kobako {
    #[cfg(target_arch = "wasm32")]
    mrb: *mut sys::mrb_state,
    /// `Kobako::RPC` base class — parent of every Service Member
    /// installed via [`Kobako::install_groups`].
    #[cfg(target_arch = "wasm32")]
    rpc_class: *mut sys::RClass,
    #[cfg(target_arch = "wasm32")]
    handle_class: *mut sys::RClass,
    #[cfg(target_arch = "wasm32")]
    service_error_class: *mut sys::RClass,
    #[cfg(target_arch = "wasm32")]
    disconnected_class: *mut sys::RClass,
    #[cfg(target_arch = "wasm32")]
    wire_error_class: *mut sys::RClass,
}

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

                // (2) Kobako::RPC base class.
                //
                // mruby's `mrb_define_class_under` accepts a NULL
                // super_ as a request to inherit from
                // `mrb->object_class` in current 3.x releases. Service
                // Member subclasses inherit from this `Kobako::RPC`
                // (see `Kobako::install_groups`), not from `Object`
                // directly, so the precise base-of-RPC choice is
                // invisible to user code.
                let rpc_class = sys::mrb_define_class_under(
                    mrb,
                    kobako_mod,
                    cstr_ptr(RPC_NAME),
                    core::ptr::null_mut(),
                );

                // (3) Singleton-class `method_missing` /
                //     `respond_to_missing?` on `Kobako::RPC`. Subclasses
                //     inherit through metaclass-chain dispatch.
                sys::mrb_define_singleton_method(
                    mrb,
                    rpc_class as *mut sys::RObject,
                    cstr_ptr(METHOD_MISSING_NAME),
                    bridges::rpc_method_missing,
                    sys::MRB_ARGS_ANY,
                );
                sys::mrb_define_singleton_method(
                    mrb,
                    rpc_class as *mut sys::RObject,
                    cstr_ptr(RESPOND_TO_MISSING_NAME),
                    bridges::rpc_respond_to_missing,
                    sys::MRB_ARGS_ANY,
                );

                // (4) `Kobako.__rpc_call__` module function with 4
                //     required args.
                sys::mrb_define_module_function(
                    mrb,
                    kobako_mod,
                    cstr_ptr(RPC_CALL_NAME),
                    bridges::kobako_rpc_call,
                    sys::mrb_args_req(4),
                );

                // (5) `Kobako::Handle` instance class.
                let handle_class = sys::mrb_define_class_under(
                    mrb,
                    kobako_mod,
                    cstr_ptr(HANDLE_NAME),
                    core::ptr::null_mut(),
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
                //     `Kobako::WireError` — all subclass `RuntimeError`.
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
                    kobako_mod,
                    cstr_ptr(WIRE_ERROR_NAME),
                    runtime_error_class,
                );

                // (7) `Kernel#puts` / `Kernel#p` shims. mruby's core
                //     `kernel.c` registers `Kernel#print` unconditionally,
                //     but `puts` / `p` only exist when the `mruby-io`
                //     mrbgem is linked in. `mruby-io` requires POSIX
                //     `<pwd.h>` and is absent from kobako's
                //     `wasm32-wasip1` allowlist, so we register both
                //     methods here and have the C bridge bodies
                //     delegate to `Kernel#print` through `mrb_funcall`.
                let kernel_mod = sys::mrb_module_get(mrb, cstr_ptr(KERNEL_NAME));
                sys::mrb_define_method(
                    mrb,
                    kernel_mod,
                    cstr_ptr(PUTS_NAME),
                    bridges::kernel_puts,
                    sys::MRB_ARGS_ANY,
                );
                sys::mrb_define_method(
                    mrb,
                    kernel_mod,
                    cstr_ptr(P_NAME),
                    bridges::kernel_p,
                    sys::MRB_ARGS_ANY,
                );

                Self {
                    mrb,
                    rpc_class,
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
                let rpc_class = sys::mrb_class_get_under(mrb, kobako_mod, cstr_ptr(RPC_NAME));
                let handle_class = sys::mrb_class_get_under(mrb, kobako_mod, cstr_ptr(HANDLE_NAME));
                let service_error_class =
                    sys::mrb_class_get_under(mrb, kobako_mod, cstr_ptr(SERVICE_ERROR_NAME));
                let disconnected_class =
                    sys::mrb_class_get_under(mrb, service_error_class, cstr_ptr(DISCONNECTED_NAME));
                let wire_error_class =
                    sys::mrb_class_get_under(mrb, kobako_mod, cstr_ptr(WIRE_ERROR_NAME));
                Self {
                    mrb,
                    rpc_class,
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

    /// Install Service Group → Member proxy classes from a Frame 1
    /// preamble. Each Group becomes a top-level Ruby module; each Member
    /// becomes a subclass of `Kobako::RPC` under its Group so the
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
                    // SAFETY: as above; `self.rpc_class` was produced by
                    // install_raw.
                    unsafe {
                        sys::mrb_define_class_under(
                            self.mrb,
                            group_mod,
                            member_cstr.as_ptr(),
                            self.rpc_class,
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

    /// Raise `Kobako::WireError` with `msg`. Diverges — `mrb_raise` does
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
        let target_cls = if ex.r#type == "disconnected" {
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

    /// Store `id_val` into a fresh `Kobako::Handle` instance's
    /// `@__kobako_id__` ivar. Used by the `Kobako::Handle#initialize`
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

    /// Read the `u32` Handle id stored in a `Kobako::Handle` instance's
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
    /// `(String, codec::Value)` pairs. Keys use `Object#to_s` (handles
    /// both Symbol and String keys); values go through
    /// [`Kobako::mrb_value_to_wire_value`].
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

    /// Convert an `mrb_value` to a kobako wire [`crate::codec::Value`]
    /// for use as an RPC argument or keyword value. Unknown types fall
    /// back to `Object#to_s` (Symbol stringifies to its name without the
    /// leading colon; other types use whatever `Object#to_s` produces).
    ///
    /// ## Why two converters
    ///
    /// This is the **RPC-path** converter — Hash arguments are decoded
    /// elsewhere into kwargs ([`Kobako::decode_hash_kwargs`]), so a
    /// stray Hash here falls through the `to_s` arm. The sibling
    /// [`Kobako::mrb_value_to_wire_outcome`] handles the **outcome-path**
    /// (the script's last-expression value) and uses `inspect` for the
    /// fallback instead. Do not unify the two: the RPC path needs the
    /// bare-name form so a Service sees `"user_42"` rather than
    /// `":user_42"`, while the outcome path is read as a display
    /// representation.
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
            // Symbol / fallback: route through `.to_s`.
            _ => Value::Str(unsafe { val.to_string(self.mrb) }),
        }
    }

    /// Convert an `mrb_value` to a kobako wire [`crate::codec::Value`]
    /// for inclusion in the outcome Result envelope. Used by
    /// `__kobako_run` to serialize the user script's last-expression
    /// value.
    ///
    /// ## Why this differs from [`Kobako::mrb_value_to_wire_value`]
    ///
    /// Unknown types fall back to `Object#inspect` rather than
    /// `Object#to_s`. The outcome envelope is read by host-side callers
    /// as a *display* representation, not an interchange value, so
    /// `inspect` (which quotes strings, shows class names, formats
    /// Array / Hash with their punctuation) is the right shape. Array
    /// and Hash currently flow through this `inspect` fallback —
    /// native wire-level Array / Hash encoding is a separate follow-up.
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
            _ => Value::Str(unsafe {
                val.call(self.mrb, cstr!("inspect"), &[])
                    .to_string(self.mrb)
            }),
        }
    }

    /// Convert a kobako wire [`crate::codec::Value`] into an `mrb_value`
    /// suitable for handing back to the mruby VM. Handle values are
    /// boxed into a fresh `Kobako::Handle` instance carrying the id
    /// (subsequent method calls on it route to the host via
    /// `Kobako.__rpc_call__`, SPEC.md B-17).
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
                Value::Array(_) | Value::Map(_) | Value::ErrEnv(_) => self.nil_value(),
            }
        }
    }

    // ----------------------------------------------------------------
    // RPC dispatch.
    // ----------------------------------------------------------------

    /// Invoke `invoke_rpc` and convert the result to an `mrb_value`. On
    /// `Response.err`, raises a matching `Kobako::ServiceError`
    /// subclass; on any other wire-layer fault, raises `Kobako::WireError`
    /// with `wire_err_msg`. Both raise paths diverge — `mrb_raise` does
    /// not return.
    #[cfg(target_arch = "wasm32")]
    pub fn dispatch_invoke(
        &self,
        target: crate::envelope::Target,
        method_name: &str,
        wire_args: &[crate::codec::Value],
        wire_kwargs: &[(String, crate::codec::Value)],
        wire_err_msg: &[u8],
    ) -> sys::mrb_value {
        use crate::rpc_client::invoke_rpc;
        match invoke_rpc(target, method_name, wire_args, wire_kwargs) {
            Ok(wire_val) => self.wire_value_to_mrb(wire_val),
            Err(crate::rpc_client::InvokeError::ServiceErr(ex)) => {
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

    // ----------------------------------------------------------------
    // `Kernel#puts` helpers.
    // ----------------------------------------------------------------

    /// Print a single mruby `String` via `Kernel#print`. Used by the
    /// `Kernel#puts` / `Kernel#p` C bridges as their atomic output
    /// operation; takes `self_` so the call site supplies the mruby
    /// receiver (the singleton object passed to the bridge).
    #[cfg(target_arch = "wasm32")]
    pub fn print_str(&self, self_: sys::mrb_value, s: sys::mrb_value) {
        // SAFETY: `self.mrb` is live; `self_` and `s` are mrb_values from
        // the same VM.
        unsafe {
            self_.call(self.mrb, cstr_ptr(PRINT_NAME), &[s]);
        }
    }

    /// `Kernel#puts` single-element body: print `arg`, append a newline
    /// when the result does not already end in one. Recurses into
    /// Arrays, matching MRI semantics one element per call.
    #[cfg(target_arch = "wasm32")]
    pub fn puts_one(&self, self_: sys::mrb_value, arg: sys::mrb_value, nl: sys::mrb_value) {
        // SAFETY: as above.
        unsafe {
            if arg.classname(self.mrb) == "Array" {
                let len = self.collection_len(arg);
                for i in 0..len {
                    let elem = sys::mrb_ary_entry(arg, i as i32);
                    self.puts_one(self_, elem, nl);
                }
                return;
            }

            let s_val = arg.call(self.mrb, cstr!("to_s"), &[]);
            self.print_str(self_, s_val);

            // Append newline unless the printed string already ended
            // with "\n". Inspect the byte at `length - 1` via Ruby
            // (`bytesize` + `getbyte`) — `mrb_str_to_cstr` would
            // mishandle embedded NULs / binary content. Both bytesize
            // and the last-byte value are small Integers; round-trip
            // through `to_s` + parse to avoid depending on a private
            // int unbox shim.
            let bytesize_val = s_val.call(self.mrb, cstr!("bytesize"), &[]);
            let bs: usize = bytesize_val.to_string(self.mrb).parse().unwrap_or(0);
            if bs == 0 {
                self.print_str(self_, nl);
                return;
            }
            let last_idx = sys::mrb_boxing_int_value(self.mrb, (bs - 1) as i32);
            let last_byte_val = s_val.call(self.mrb, cstr!("getbyte"), &[last_idx]);
            let lb: i32 = last_byte_val.to_string(self.mrb).parse().unwrap_or(0);
            if lb != 10 {
                self.print_str(self_, nl);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_groups_error_variants_are_distinct() {
        // Compile-time / debug-form check: variants are not accidentally
        // collapsed. The two reasons land on distinct error envelopes in
        // `__kobako_run`, so they must remain distinguishable.
        assert_ne!(
            InstallGroupsError::NulInGroupName,
            InstallGroupsError::NulInMemberName
        );
    }

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
