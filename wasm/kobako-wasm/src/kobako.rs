//! Kobako runtime — installs the Kobako module surface onto an mruby VM
//! and exposes the installed class handles to the rest of the guest.
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
//! The shape mirrors `magnus::Ruby::define_class` for CRuby: a domain
//! type owns the registration vocabulary, and a separate VM-handle type
//! owns the language runtime. The C-bridge callbacks in [`crate::boot`]
//! remain plain `unsafe extern "C" fn` (mruby's ABI requires it), but
//! `Kobako` is the boundary that decides what gets registered and keeps
//! the class handles needed by the dispatch layer.
//!
//! ## Lifecycle
//!
//! [`Kobako::install`] is called once per `__kobako_run` invocation,
//! immediately after [`Mrb::open`]. It registers every boot-time entity
//! and returns a [`Kobako<'a>`] borrowing the [`Mrb`]. The returned
//! handle is then used to drive the Frame 1 preamble through
//! [`Kobako::install_groups`].
//!
//! Subsequent items will grow methods that today still live in
//! `crate::boot` (e.g. `raise_wire_error`, `raise_service_error`,
//! `wire_value_to_mrb`) and migrate the C-bridges to obtain a `Kobako`
//! via [`Kobako::resolve`] from the raw `*mut mrb_state` they receive.

#[cfg(target_arch = "wasm32")]
use crate::mruby::sys;
#[cfg(target_arch = "wasm32")]
use crate::mruby::value::cstr_ptr;
use crate::mruby::Mrb;

#[cfg(target_arch = "wasm32")]
const KOBAKO_NAME: &[u8] = b"Kobako\0";
#[cfg(target_arch = "wasm32")]
const RPC_NAME: &[u8] = b"RPC\0";

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

/// Handle to a Kobako runtime installed on a live mruby VM. Borrows the
/// [`Mrb`] for the lifetime of the handle so that the borrow checker
/// proves the VM is still open.
pub struct Kobako<'a> {
    mrb: &'a Mrb,
    /// `Kobako::RPC` base class — parent of every Service Member
    /// subclass installed via [`Kobako::install_groups`]. The `Kobako`
    /// module handle itself is intentionally not cached here yet; future
    /// methods that need it (raise_wire_error / raise_service_error)
    /// will introduce the field together with their first use, so the
    /// type stays free of fields without an active consumer.
    #[cfg(target_arch = "wasm32")]
    rpc_class: *mut sys::RClass,
}

impl<'a> Kobako<'a> {
    /// Install the Kobako runtime onto `mrb`: registers `Kobako`,
    /// `Kobako::RPC`, `Kobako::Handle`, `Kobako::ServiceError`,
    /// `Kobako::ServiceError::Disconnected`, `Kobako::WireError`, and
    /// the `Kernel#puts` / `Kernel#p` shims.
    ///
    /// This commit delegates to the existing
    /// [`crate::boot::mrb_kobako_init`] body and then resolves the
    /// resulting class handles. Folding every individual C API call into
    /// methods on `Kobako` is a follow-up; this commit only establishes
    /// the boundary so callers stop reaching for the raw FFI directly.
    pub fn install(mrb: &'a Mrb) -> Self {
        // SAFETY: `mrb` is a live, non-closed state per the `&Mrb`
        // borrow; `mrb_kobako_init` is safe to call against any live
        // state per its own contract.
        unsafe { crate::boot::mrb_kobako_init(mrb.as_ptr()) };
        Self::resolve(mrb)
    }

    /// Look up the class handles produced by a prior [`Kobako::install`]
    /// call without re-running registration. Intended for the future
    /// C-bridge migration: bridges receive a raw `*mut mrb_state` from
    /// mruby, wrap it in a borrowed `Mrb` view, and resolve the Kobako
    /// classes through this constructor instead of re-walking the C API
    /// at every call site.
    pub fn resolve(mrb: &'a Mrb) -> Self {
        #[cfg(target_arch = "wasm32")]
        {
            // SAFETY: `mrb` is a live state. `mrb_define_module` is
            // idempotent — it returns the existing module if already
            // registered — and `mrb_class_get_under` returns the
            // already-registered `Kobako::RPC` class produced by
            // `install`. The intermediate `Kobako` module handle is not
            // cached on `Self` until a method needs it (Stage D).
            let kobako_mod = unsafe { sys::mrb_define_module(mrb.as_ptr(), cstr_ptr(KOBAKO_NAME)) };
            let rpc_class =
                unsafe { sys::mrb_class_get_under(mrb.as_ptr(), kobako_mod, cstr_ptr(RPC_NAME)) };
            Self { mrb, rpc_class }
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            Self { mrb }
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
                // SAFETY: `mrb` is alive; the group name is NUL-terminated
                // by `CString::new`.
                let group_mod =
                    unsafe { sys::mrb_define_module(self.mrb.as_ptr(), group_cstr.as_ptr()) };
                for member_name in members {
                    let member_cstr = std::ffi::CString::new(member_name.as_str())
                        .map_err(|_| InstallGroupsError::NulInMemberName)?;
                    // SAFETY: as above; `self.rpc_class` was produced by
                    // `mrb_define_class_under` during install.
                    unsafe {
                        sys::mrb_define_class_under(
                            self.mrb.as_ptr(),
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

    /// Borrow the underlying [`Mrb`] for call sites that still reach for
    /// the raw FFI via [`Mrb::as_ptr`].
    #[inline]
    pub fn mrb(&self) -> &Mrb {
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
