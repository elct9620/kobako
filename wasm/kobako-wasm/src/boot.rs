//! Guest Binary boot — Rust-side mruby C API registrations.
//!
//! This module replaces the previous `boot.rb` + `include_str!`
//! mechanism with the **mruby C API path**. No Ruby text is loaded into
//! the mruby VM at boot time; instead, every foundational entity is
//! registered directly via C API calls:
//!
//!   1. `Kobako` module — `mrb_define_module(mrb, "Kobako")`.
//!   2. `Kobako::RPC` base class — `mrb_define_class_under(mrb,
//!      kobako_mod, "RPC", mrb->object_class)`. Each Service Member
//!      (e.g. `MyService::KV`) is, at runtime, a *subclass* of
//!      `Kobako::RPC` created by the Frame 1 preamble — they inherit
//!      the singleton-class `method_missing` installed here.
//!   3. `Kobako.__rpc_call__(target, method, args, kwargs)` —
//!      `mrb_define_module_function(mrb, kobako_mod, "__rpc_call__",
//!      c_fn, MRB_ARGS_REQ(4))`. The four-arg module function is the
//!      single dispatch entry point shared by both `Kobako::RPC`
//!      subclasses (path target) and `Kobako::Handle` instances (handle
//!      target — wire ext 0x01, see SPEC.md "ext 0x01 — Capability
//!      Handle").
//!   4. `Kobako::Handle` class — instance subclass of `Object` carrying a
//!      Handle id in `@__kobako_id__`. `initialize`, `method_missing`
//!      and `respond_to_missing?` are all C bridges.
//!   5. `Kobako::ServiceError < RuntimeError` and
//!      `Kobako::WireError < RuntimeError` — raised by the Rust bridges
//!      via `mrb_raise`.
//!   6. `Kernel#puts` and `Kernel#p` — mruby core only registers
//!      `Kernel#print`; `puts`/`p` are normally provided by the
//!      `mruby-io` gem, which depends on POSIX `<pwd.h>` and is absent
//!      from the wasm32-wasip1 allowlist (`build_config/wasi.rb`). We
//!      re-implement them as C bridges that delegate to `Kernel#print`
//!      via `mrb_funcall`.
//!
//! `mrb_load_string` / `mrb_load_nstring` is intentionally not used for
//! the boot/preload phase — every entity above is defined via C API.
//! The only `mrb_load_nstring` call sites in the guest are inside
//! `__kobako_run` for evaluating Frame 2 (the user script). This file never inspects
//! or constructs `mrb_value` payloads; it forwards them through the FFI
//! shims in `crate::mruby::sys`.
//!
//! ## Lifecycle
//!
//! `mrb_kobako_init(mrb)` is called once per `__kobako_run` entry,
//! immediately after the mruby state is created and before the Frame 1
//! preamble executes (which depends on `Kobako::RPC` being available
//! to be `super_` of each Service Member subclass). The wasm-side
//! lib.rs `__kobako_run` body wires this into the reactor flow once
//! item #16 lands.
//!
//! ## What this module is NOT responsible for
//!
//! The original `boot.rb` had three responsibilities — those have all
//! moved:
//!
//!   * "State init / capture $stdout/$stderr" — stdout/stderr are
//!     **user-observable channels** delivered by wasi fds 1/2. The host
//!     side reads them through `Sandbox#stdout` / `Sandbox#stderr`. No
//!     mruby-side capture is needed.
//!   * "Service::Group::Member proxy install" — that proxy *is* the
//!     `Kobako::RPC` subclass mechanism this module registers; the
//!     Frame 1 preamble (item #11 / future) creates the subclasses.
//!   * "I/O drain hook" — also obsolete: WASI flushes fds 1/2
//!     synchronously through the host's import; there is no
//!     mruby-level buffering to drain.
//!
//! All three of those responsibilities are accounted for by the
//! `__kobako_run` body and the host-side ABI; none of them require an
//! mruby-VM-side artifact.

use crate::cstr;
use crate::mruby::sys;

// All registration-time C strings and value helpers now live with their
// owner in `crate::kobako`; this file only keeps the `unsafe extern "C"
// fn` C-bridges that mruby registers as method bodies.

// --------------------------------------------------------------------
// Public entry point.
// --------------------------------------------------------------------

/// Thin shim that forwards to [`crate::kobako::Kobako::install_raw`].
///
/// Retained as a public entry point so external callers / tests that
/// reach for `mrb_kobako_init(mrb)` continue to work; the registration
/// body itself lives in `crate::kobako` now (see the module docs there
/// for the rationale behind the `Kobako` boundary).
///
/// # Safety
///
/// `mrb` must be a valid `mrb_state *` returned by `mrb_open` (or
/// equivalent) and not yet closed.
pub unsafe fn mrb_kobako_init(mrb: *mut sys::mrb_state) {
    let _ = unsafe { crate::kobako::Kobako::install_raw(mrb) };
}

// --------------------------------------------------------------------
// C-callable shims registered above.
// --------------------------------------------------------------------
//
// All three are registered with mruby; the dispatch chain at runtime
// is:
//
//   user_script:    MyService::KV.get(:user_42)
//        │
//        │ (no instance method named `get`; class-level dispatch falls
//        │  through to singleton-class `method_missing`, inherited
//        │  from `Kobako::RPC.singleton_class`)
//        ▼
//   rpc_method_missing(mrb, self=KV.class)
//        │
//        │ (extract method symbol + args; build kwargs hash from
//        │  trailing Hash if present; resolve target string via
//        │  `mrb_class_name(mrb, mrb_class_ptr(self))`)
//        ▼
//   forwards to Kobako.__rpc_call__(target, method, args, kwargs)
//        │
//        ▼
//   kobako_rpc_call(mrb, self=Kobako module)
//        │
//        ▼
//   crate::rpc_client::invoke_rpc(...)
//
// The full chain lands incrementally:
//   * Item #29 (this item): registrations + thin C bridges that
//     surface a clear `Kobako::WireError` until the bodies are wired.
//   * Item #11 / #16: full body wiring `mrb_get_args`, marshalling to
//     `crate::codec::Value`, calling `invoke_rpc`, and decoding the
//     response into a fresh `mrb_value` via the boxing macros in the
//     mruby::sys shim layer.
//
// At this item, the bridge functions are deliberately minimal: they
// raise `Kobako::WireError` with a "not yet wired" message. That keeps
// the boot mechanism end-to-end testable from item #16 onwards (the
// caller sees a structured error rather than a wasm trap) while
// avoiding a body-write that depends on the not-yet-bound boxing
// macros.

/// `Kobako.__rpc_call__(target, method, args, kwargs)` C bridge.
///
/// Receives four positional args assembled by `rpc_method_missing`:
///   - `target`: String path (e.g. `"MyService::KV"`) or Handle integer.
///   - `method`: String method name.
///   - `args`: Array of positional Wire values.
///   - `kwargs`: Hash of String key → Wire value pairs.
///
/// Delegates to `crate::rpc_client::invoke_rpc`. On success, returns the
/// wire-decoded mruby value. On service error, raises `Kobako::ServiceError`.
/// On wire error, raises `Kobako::WireError`.
#[allow(unused_variables)]
pub(crate) unsafe extern "C" fn kobako_rpc_call(
    mrb: *mut sys::mrb_state,
    _self_: sys::mrb_value,
) -> sys::mrb_value {
    #[cfg(target_arch = "wasm32")]
    {
        use crate::envelope::Target;

        // SAFETY: `mrb` is live by the bridge contract (mruby invoked us
        // through a registration done at install time). Construct a
        // `Kobako` token once at the top of the bridge — everything
        // below uses safe methods.
        let kobako = unsafe { crate::kobako::Kobako::resolve_raw(mrb) };

        // Unpack 4 required positional args: target, method, args_ary, kwargs_hash
        let mut target_val = sys::mrb_value::zeroed();
        let mut method_val = sys::mrb_value::zeroed();
        let mut args_ary = sys::mrb_value::zeroed();
        let mut kwargs_hash = sys::mrb_value::zeroed();
        unsafe {
            sys::mrb_get_args(
                mrb,
                cstr!("oooo"),
                &mut target_val as *mut sys::mrb_value,
                &mut method_val as *mut sys::mrb_value,
                &mut args_ary as *mut sys::mrb_value,
                &mut kwargs_hash as *mut sys::mrb_value,
            );
        }

        // Decode target: String path or Kobako::Handle instance.
        let target = match unsafe { target_val.classname(mrb) } {
            "Kobako::Handle" => Target::Handle(kobako.extract_handle_id(target_val)),
            _ => Target::Path(unsafe { target_val.to_string(mrb) }),
        };

        // Decode method name string. NULL pointer (not just an empty
        // string) is the wire violation — preserve the distinction by
        // checking the raw `mrb_str_to_cstr` pointer instead of going
        // through `to_string`, which collapses NULL into "".
        let method_ptr = unsafe { sys::mrb_str_to_cstr(mrb, method_val) };
        let method_name = if method_ptr.is_null() {
            // SAFETY: bridge frame — mruby unwinds through `mrb_raise`.
            unsafe { kobako.raise_wire_error(b"RPC method name is null\0") };
        } else {
            unsafe { core::ffi::CStr::from_ptr(method_ptr) }
                .to_str()
                .unwrap_or("")
        };

        // Decode positional args from the Array.
        let args_len = kobako.collection_len(args_ary);
        let mut wire_args = Vec::with_capacity(args_len);
        for i in 0..args_len {
            let elem = unsafe { sys::mrb_ary_entry(args_ary, i as i32) };
            wire_args.push(kobako.mrb_value_to_wire_value(elem));
        }

        // Decode kwargs from the Hash. Skip silently when kwargs_hash is
        // not actually a Hash (defensive — `oooo` unpack accepts any
        // object).
        let mut wire_kwargs = Vec::new();
        if unsafe { kwargs_hash.classname(mrb) } == "Hash" {
            kobako.decode_hash_kwargs(kwargs_hash, &mut wire_kwargs);
        }

        kobako.dispatch_invoke(
            target,
            method_name,
            &wire_args,
            &wire_kwargs,
            b"RPC wire error during invoke_rpc\0",
        )
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        sys::mrb_value::zeroed()
    }
}

/// `Kobako::RPC.method_missing(name, *args)` C bridge — singleton-class
/// level, so `self` is the class object (e.g. `MyService::KV`).
///
/// Extracts:
///   - `target` = full class name from `mrb_class_name(mrb, mrb_class_ptr(self))`
///   - `method` = first arg (Symbol → String)
///   - `args`   = rest args (positional), last arg absorbed into kwargs if Hash
///   - `kwargs` = trailing Hash arg (if last positional is a Hash)
///
/// Forwards to `kobako_rpc_call` via `invoke_rpc`.
#[allow(unused_variables)]
pub(crate) unsafe extern "C" fn rpc_method_missing(
    mrb: *mut sys::mrb_state,
    self_: sys::mrb_value,
) -> sys::mrb_value {
    #[cfg(target_arch = "wasm32")]
    {
        use crate::envelope::Target;

        // SAFETY: as above.
        let kobako = unsafe { crate::kobako::Kobako::resolve_raw(mrb) };

        // Unpack: method_name_sym + rest args.
        let mut method_sym: sys::mrb_sym = 0;
        let mut rest_ptr: *const sys::mrb_value = core::ptr::null();
        let mut rest_len: core::ffi::c_int = 0;
        unsafe {
            sys::mrb_get_args(
                mrb,
                cstr!("n*"),
                &mut method_sym as *mut sys::mrb_sym,
                &mut rest_ptr as *mut *const sys::mrb_value,
                &mut rest_len as *mut core::ffi::c_int,
            );
        }

        // Get the target class name. `mrb_class_ptr(val)` is the C macro
        // `((struct RClass*)(mrb_ptr(val)))`. On wasm32 with
        // MRB_WORDBOX_NO_INLINE_FLOAT + MRB_INT32, `mrb_ptr(val)` is the
        // raw pointer stored in the lower bits of `mrb_value.w` for
        // object-tagged values — i.e. `self_.w as *mut RClass`. We
        // implement the macro inline to avoid declaring it as an extern
        // "C" fn (it is a macro, not a function, so an FFI declaration
        // would produce an unresolved wasm import).
        let class_ptr = self_.w as *mut sys::RClass;
        let class_name_ptr = unsafe { sys::mrb_class_name(mrb, class_ptr) };
        let target_str = if class_name_ptr.is_null() {
            // SAFETY: bridge frame.
            unsafe { kobako.raise_wire_error(b"RPC target class name is null\0") };
        } else {
            unsafe { core::ffi::CStr::from_ptr(class_name_ptr) }
                .to_str()
                .unwrap_or("")
        };

        // Get the method name string from the symbol.
        let method_name_ptr = unsafe { sys::mrb_sym_name(mrb, method_sym) };
        let method_name = if method_name_ptr.is_null() {
            unsafe { kobako.raise_wire_error(b"RPC method symbol name is null\0") };
        } else {
            unsafe { core::ffi::CStr::from_ptr(method_name_ptr) }
                .to_str()
                .unwrap_or("")
        };

        // Build args and kwargs from rest_ptr[0..rest_len].
        let rest: &[sys::mrb_value] = if rest_len > 0 && !rest_ptr.is_null() {
            // SAFETY: mruby passes a valid array on `n*` unpack.
            unsafe { core::slice::from_raw_parts(rest_ptr, rest_len as usize) }
        } else {
            &[]
        };

        let (wire_args, wire_kwargs) = kobako.unpack_args_kwargs(rest);
        let target = Target::Path(target_str.to_string());

        kobako.dispatch_invoke(
            target,
            method_name,
            &wire_args,
            &wire_kwargs,
            b"RPC wire error\0",
        )
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        sys::mrb_value::zeroed()
    }
}

/// `Kobako::Handle#initialize(id)` C bridge.
///
/// Stores the Handle integer id in the `@__kobako_id__` instance variable.
/// Called by `mrb_obj_new` when creating a `Kobako::Handle` instance.
#[allow(unused_variables)]
pub(crate) unsafe extern "C" fn handle_initialize(
    mrb: *mut sys::mrb_state,
    self_: sys::mrb_value,
) -> sys::mrb_value {
    #[cfg(target_arch = "wasm32")]
    {
        // SAFETY: bridge contract.
        let kobako = unsafe { crate::kobako::Kobako::resolve_raw(mrb) };

        let mut id_val = sys::mrb_value::zeroed();
        unsafe {
            sys::mrb_get_args(mrb, cstr!("o"), &mut id_val as *mut sys::mrb_value);
        }
        kobako.set_handle_id(self_, id_val);
    }
    sys::mrb_value::zeroed()
}

/// `Kobako::Handle#method_missing(name, *args)` C bridge.
///
/// Forwards every method call on a Handle instance to the host via
/// `Kobako.__rpc_call__(id, method_name, args, kwargs)` with the Handle id
/// as an integer target (SPEC.md B-17 — Handle chaining).
#[allow(unused_variables)]
pub(crate) unsafe extern "C" fn handle_method_missing(
    mrb: *mut sys::mrb_state,
    self_: sys::mrb_value,
) -> sys::mrb_value {
    #[cfg(target_arch = "wasm32")]
    {
        use crate::envelope::Target;

        // SAFETY: bridge contract.
        let kobako = unsafe { crate::kobako::Kobako::resolve_raw(mrb) };

        // Unpack: method_name_sym + rest args.
        let mut method_sym: sys::mrb_sym = 0;
        let mut rest_ptr: *const sys::mrb_value = core::ptr::null();
        let mut rest_len: core::ffi::c_int = 0;
        unsafe {
            sys::mrb_get_args(
                mrb,
                cstr!("n*"),
                &mut method_sym as *mut sys::mrb_sym,
                &mut rest_ptr as *mut *const sys::mrb_value,
                &mut rest_len as *mut core::ffi::c_int,
            );
        }

        // Retrieve the Handle id from @__kobako_id__.
        let handle_id = kobako.extract_handle_id(self_);

        // Get the method name string from the symbol.
        let method_name_ptr = unsafe { sys::mrb_sym_name(mrb, method_sym) };
        let method_name = if method_name_ptr.is_null() {
            unsafe { kobako.raise_wire_error(b"Handle method symbol name is null\0") };
        } else {
            unsafe { core::ffi::CStr::from_ptr(method_name_ptr) }
                .to_str()
                .unwrap_or("")
        };

        // Build args and kwargs from rest_ptr[0..rest_len].
        let rest: &[sys::mrb_value] = if rest_len > 0 && !rest_ptr.is_null() {
            unsafe { core::slice::from_raw_parts(rest_ptr, rest_len as usize) }
        } else {
            &[]
        };

        let (wire_args, wire_kwargs) = kobako.unpack_args_kwargs(rest);
        let target = Target::Handle(handle_id);

        kobako.dispatch_invoke(
            target,
            method_name,
            &wire_args,
            &wire_kwargs,
            b"RPC wire error (Handle dispatch)\0",
        )
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        sys::mrb_value::zeroed()
    }
}

/// `Kobako::RPC.respond_to_missing?(name, include_private)` C bridge.
///
/// Always returns `true` — every method call on a Service Member class
/// is dispatched through `method_missing` to the host. Returning `true`
/// here ensures `respond_to?` checks succeed for mruby code that probes
/// Service Member capabilities.
#[allow(unused_variables)]
pub(crate) unsafe extern "C" fn rpc_respond_to_missing(
    _mrb: *mut sys::mrb_state,
    _self_: sys::mrb_value,
) -> sys::mrb_value {
    #[cfg(target_arch = "wasm32")]
    {
        // SAFETY: bridge contract.
        let kobako = unsafe { crate::kobako::Kobako::resolve_raw(_mrb) };
        kobako.r#true()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        sys::mrb_value::zeroed()
    }
}

// --------------------------------------------------------------------
// `Kernel#puts` / `Kernel#p` C bridges.
// --------------------------------------------------------------------
//
// Both bridges delegate to `Kernel#print` via `mrb_funcall`. We take the
// `mrb_funcall` route rather than calling mruby's internal `mrb_print_m`
// directly because `mrb_print_m` is not part of the public mruby C API
// (it's a static handler installed in `kernel.c`).
//
// `puts` semantics matched against MRI:
//   * No args             → emit a single "\n".
//   * `Array` arg         → recurse into each element (one line per
//                            non-Array element, preserving order).
//   * Other args          → call `.to_s`, print, append "\n" unless the
//                            string already ends in "\n".
//
// `p` semantics matched against MRI:
//   * Each arg            → print `arg.inspect` followed by "\n".
//   * Returns the single arg if `argc == 1`, the args Array otherwise,
//     `nil` when called with no arguments.

/// `Kernel#puts(*args)` C bridge.
#[allow(unused_variables)]
pub(crate) unsafe extern "C" fn kernel_puts(
    mrb: *mut sys::mrb_state,
    self_: sys::mrb_value,
) -> sys::mrb_value {
    #[cfg(target_arch = "wasm32")]
    {
        // SAFETY: bridge contract.
        let kobako = unsafe { crate::kobako::Kobako::resolve_raw(mrb) };

        let mut args_ptr: *const sys::mrb_value = core::ptr::null();
        let mut args_len: core::ffi::c_int = 0;
        unsafe {
            sys::mrb_get_args(
                mrb,
                cstr!("*"),
                &mut args_ptr as *mut *const sys::mrb_value,
                &mut args_len as *mut core::ffi::c_int,
            );
        }

        let nl = unsafe { sys::mrb_str_new(mrb, b"\n".as_ptr() as *const core::ffi::c_char, 1) };

        if args_len == 0 {
            kobako.print_str(self_, nl);
            return kobako.nil();
        }

        let args = unsafe { core::slice::from_raw_parts(args_ptr, args_len as usize) };
        for &arg in args {
            kobako.puts_one(self_, arg, nl);
        }
        kobako.nil()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        sys::mrb_value::zeroed()
    }
}

/// `Kernel#p(*args)` C bridge.
#[allow(unused_variables)]
pub(crate) unsafe extern "C" fn kernel_p(
    mrb: *mut sys::mrb_state,
    self_: sys::mrb_value,
) -> sys::mrb_value {
    #[cfg(target_arch = "wasm32")]
    {
        // SAFETY: bridge contract.
        let kobako = unsafe { crate::kobako::Kobako::resolve_raw(mrb) };

        let mut args_ptr: *const sys::mrb_value = core::ptr::null();
        let mut args_len: core::ffi::c_int = 0;
        unsafe {
            sys::mrb_get_args(
                mrb,
                cstr!("*"),
                &mut args_ptr as *mut *const sys::mrb_value,
                &mut args_len as *mut core::ffi::c_int,
            );
        }

        let nl = unsafe { sys::mrb_str_new(mrb, b"\n".as_ptr() as *const core::ffi::c_char, 1) };
        let args = unsafe { core::slice::from_raw_parts(args_ptr, args_len as usize) };
        for &arg in args {
            let insp = unsafe { arg.call(mrb, cstr!("inspect"), &[]) };
            kobako.print_str(self_, insp);
            kobako.print_str(self_, nl);
        }

        match args_len {
            0 => kobako.nil(),
            1 => args[0],
            _ => unsafe { sys::mrb_ary_new_from_values(mrb, args_len, args_ptr) },
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        sys::mrb_value::zeroed()
    }
}

// --------------------------------------------------------------------
// Tests — host target.
// --------------------------------------------------------------------
//
// On host the FFI calls are absent (`#[cfg(target_arch = "wasm32")]`).
// What we *can* test cheaply is that the function items compile with
// the documented signatures and that the C-string constants are well
// formed (NUL-terminated, ASCII). C API signature regressions surface
// as compile errors in `mruby::sys` — we don't need duplicate runtime
// asserts.

#[cfg(test)]
mod tests {
    use super::*;

    // Registration-time C-string constants moved to `crate::kobako`; the
    // C-bridges remaining here use literal byte slices at their call
    // sites, exercised end-to-end by `data/kobako.wasm`. A duplicate
    // host-target byte-pattern check would be pure churn.

    #[test]
    fn mrb_kobako_init_is_safe_no_op_on_host() {
        // On host target the function body short-circuits via the
        // `target_arch = "wasm32"` cfg, so passing a null `mrb` is
        // safe. This guard documents the host-side contract: the
        // function exists with a stable signature and is a true no-op
        // when the FFI cannot reach mruby.
        unsafe { mrb_kobako_init(core::ptr::null_mut()) };
    }

    #[test]
    fn c_bridges_have_mrb_func_t_signature() {
        // Compile-time signature check — these `let` bindings fail to
        // compile if the bridge functions drift from `mrb_func_t`.
        // This is the host-target compile-time replacement for the
        // mruby-link-level signature check.
        let _f1: sys::mrb_func_t = kobako_rpc_call;
        let _f2: sys::mrb_func_t = rpc_method_missing;
        let _f3: sys::mrb_func_t = rpc_respond_to_missing;
        let _f4: sys::mrb_func_t = handle_initialize;
        let _f5: sys::mrb_func_t = handle_method_missing;
    }
}
