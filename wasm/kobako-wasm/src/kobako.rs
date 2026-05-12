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
    /// installed via [`Kobako::install_groups`]. The `Kobako` module
    /// handle itself is intentionally not cached: the only consumers
    /// that need it (D-2 value-conversion helpers) will introduce the
    /// field together with their first use.
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
    /// a raw pointer from mruby itself (currently the C bridge in
    /// [`crate::boot::mrb_kobako_init`]).
    #[cfg_attr(not(target_arch = "wasm32"), allow(unused_variables))]
    pub unsafe fn install_raw(mrb: *mut sys::mrb_state) -> Self {
        #[cfg(target_arch = "wasm32")]
        {
            use crate::boot as bridges;

            // (1) Kobako module.
            let kobako_mod = sys::mrb_define_module(mrb, cstr_ptr(KOBAKO_NAME));

            // (2) Kobako::RPC base class.
            //
            // The super-class is `mrb->object_class`. mruby's
            // `mrb_define_class_under` accepts a NULL super_ as a
            // request to inherit from Object in current 3.x releases.
            // Service Member subclasses inherit from this `Kobako::RPC`
            // (see `Kobako::install_groups`), not from Object directly,
            // so the precise base-of-RPC choice is invisible to user
            // code.
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

            // (4) `Kobako.__rpc_call__` module function with 4 required
            //     args.
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

            // (6) `Kobako::ServiceError` / `Kobako::ServiceError::Disconnected`
            //     / `Kobako::WireError` — all subclass `RuntimeError`.
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
            //     but `puts` / `p` only exist when the `mruby-io` mrbgem
            //     is linked in. `mruby-io` requires POSIX `<pwd.h>` and
            //     is absent from kobako's `wasm32-wasip1` allowlist, so
            //     we register both methods here and have the C bridge
            //     bodies delegate to `Kernel#print` through
            //     `mrb_funcall`.
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
            // SAFETY of every call below: `mrb` is live; `mrb_define_module`
            // is idempotent (returns the existing module if already
            // registered); `mrb_class_get_under` returns the already
            // -registered class produced by `install_raw`.
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

    /// `Kobako::Handle` class handle — exposed so C-bridges that still
    /// live in [`crate::boot`] can call `mrb_obj_new` against it. This
    /// accessor disappears once the value-conversion helpers migrate.
    #[cfg(target_arch = "wasm32")]
    pub fn handle_class(&self) -> *mut sys::RClass {
        self.handle_class
    }

    /// Raw mruby state pointer. Used by helpers that still talk to the
    /// C API directly.
    #[cfg(target_arch = "wasm32")]
    pub fn as_ptr(&self) -> *mut sys::mrb_state {
        self.mrb
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
}
