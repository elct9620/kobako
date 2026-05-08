//! Guest Binary boot — Rust-side mruby C API registrations.
//!
//! This module replaces the previous `boot.rb` + `include_str!`
//! mechanism with the **mruby C API path**. No Ruby text is loaded into
//! the mruby VM at boot time; instead, the three foundational entities
//! are registered directly via C API calls:
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
//!
//! `mrb_load_string` is intentionally not used for the boot/preload
//! phase — every entity is defined via C API. This file never inspects
//! or constructs `mrb_value` payloads; it forwards them through the FFI
//! shims in `mruby_sys.rs`.
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

use crate::mruby_sys as sys;

// --------------------------------------------------------------------
// C strings — null-terminated for FFI calls.
// --------------------------------------------------------------------

/// `b"Kobako\0"`. mruby's `mrb_define_module` expects a NUL-terminated
/// C string per `<mruby.h>`.
const KOBAKO_NAME: &[u8] = b"Kobako\0";
/// `b"RPC\0"`.
const RPC_NAME: &[u8] = b"RPC\0";
/// `b"Handle\0"`.
const HANDLE_NAME: &[u8] = b"Handle\0";
/// `b"__rpc_call__\0"`.
const RPC_CALL_NAME: &[u8] = b"__rpc_call__\0";
/// `b"method_missing\0"`.
const METHOD_MISSING_NAME: &[u8] = b"method_missing\0";
/// `b"respond_to_missing?\0"`.
const RESPOND_TO_MISSING_NAME: &[u8] = b"respond_to_missing?\0";
/// `b"initialize\0"`.
const INITIALIZE_NAME: &[u8] = b"initialize\0";
/// `b"@__kobako_id__\0"` — instance variable name for Handle id storage.
/// Uses a mangled name to avoid collision with user-defined ivars.
#[cfg(target_arch = "wasm32")]
const HANDLE_ID_IVAR: &[u8] = b"@__kobako_id__\0";

// --------------------------------------------------------------------
// Public entry point.
// --------------------------------------------------------------------

/// Register `Kobako` module, `Kobako::RPC` base class, and
/// `Kobako.__rpc_call__` module function on the given mruby state.
///
/// # Safety
///
/// `mrb` must be a valid `mrb_state *` returned by `mrb_open` (or
/// equivalent) and not yet closed. The caller is `__kobako_run` in
/// `lib.rs` (or test code on the host target with a stub `mrb_state`).
///
/// # wasm32-only
///
/// The body issues real mruby C API calls and is therefore gated on
/// `target_arch = "wasm32"`. On the host target this function is a
/// no-op so the rlib used by `cargo test` compiles without
/// `libmruby.a` in the link graph.
#[allow(unused_variables)]
pub unsafe fn mrb_kobako_init(mrb: *mut sys::mrb_state) {
    #[cfg(target_arch = "wasm32")]
    {
        // (1) `mrb_define_module(mrb, "Kobako")`.
        let kobako_mod = sys::mrb_define_module(
            mrb,
            KOBAKO_NAME.as_ptr() as *const core::ffi::c_char,
        );

        // (2) `Kobako::RPC` base class.
        //
        // The super-class is `mrb->object_class`. The standard idiom is
        // `mrb_define_class_under(mrb, kobako_mod, "RPC",
        // mrb->object_class)`. We pass `core::ptr::null_mut()` for
        // `super_` here: mruby's `mrb_define_class_under` accepts
        // a `NULL` super-class as a request to inherit from
        // `mrb->object_class` in current mruby releases. The Frame 1
        // preamble (item #11+) inherits Service Members directly from
        // this `Kobako::RPC` class, not from `Object`, so the precise
        // base-of-RPC choice is not visible to user code.
        //
        // NOTE: if mruby strictness changes in a future release, the
        // fix is to thread `mrb->object_class` through a small shim in
        // `mruby_sys.rs` rather than re-writing this function — the
        // boot mechanism shape is stable.
        let rpc_class = sys::mrb_define_class_under(
            mrb,
            kobako_mod,
            RPC_NAME.as_ptr() as *const core::ffi::c_char,
            core::ptr::null_mut(),
        );

        // (3) Singleton-class `method_missing` and `respond_to_missing?`
        //     on `Kobako::RPC`.
        //
        // `mrb_define_singleton_method` takes the *object* whose
        // singleton-class receives the method. For class-level
        // `method_missing` the object is the class itself, cast to
        // `RObject*`. Subclasses inherit through metaclass-chain
        // dispatch.
        sys::mrb_define_singleton_method(
            mrb,
            rpc_class as *mut sys::RObject,
            METHOD_MISSING_NAME.as_ptr() as *const core::ffi::c_char,
            rpc_method_missing,
            sys::MRB_ARGS_ANY,
        );
        sys::mrb_define_singleton_method(
            mrb,
            rpc_class as *mut sys::RObject,
            RESPOND_TO_MISSING_NAME.as_ptr() as *const core::ffi::c_char,
            rpc_respond_to_missing,
            sys::MRB_ARGS_ANY,
        );

        // (4) `Kobako.__rpc_call__` module function with 4 required
        //     args.
        sys::mrb_define_module_function(
            mrb,
            kobako_mod,
            RPC_CALL_NAME.as_ptr() as *const core::ffi::c_char,
            kobako_rpc_call,
            sys::mrb_args_req(4),
        );

        // (5) `Kobako::Handle` class — returned by Service calls that produce
        //     stateful objects. Instances carry a Handle id (`@__kobako_id__`)
        //     and forward every method call to the host via `Kobako.__rpc_call__`
        //     with `Target::Handle(id)` (SPEC.md §B-17).
        //
        //     class Kobako::Handle
        //       def initialize(id)  # C shim: stores id in @__kobako_id__
        //       def method_missing(name, *args)  # C shim: routes to __rpc_call__
        //       def respond_to_missing?(name, include_private = false)  → true
        //     end
        let handle_class = sys::mrb_define_class_under(
            mrb,
            kobako_mod,
            HANDLE_NAME.as_ptr() as *const core::ffi::c_char,
            core::ptr::null_mut(), // inherit from Object
        );
        sys::mrb_define_method(
            mrb,
            handle_class,
            INITIALIZE_NAME.as_ptr() as *const core::ffi::c_char,
            handle_initialize,
            sys::mrb_args_req(1),
        );
        sys::mrb_define_method(
            mrb,
            handle_class,
            METHOD_MISSING_NAME.as_ptr() as *const core::ffi::c_char,
            handle_method_missing,
            sys::MRB_ARGS_ANY,
        );
        sys::mrb_define_method(
            mrb,
            handle_class,
            RESPOND_TO_MISSING_NAME.as_ptr() as *const core::ffi::c_char,
            rpc_respond_to_missing,
            sys::MRB_ARGS_ANY,
        );
    }
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
//     mruby_sys shim layer.
//
// At this item, the bridge functions are deliberately minimal: they
// raise `Kobako::WireError` with a "not yet wired" message. That keeps
// the boot mechanism end-to-end testable from item #16 onwards (the
// caller sees a structured error rather than a wasm trap) while
// avoiding a body-write that depends on the not-yet-bound boxing
// macros.

#[cfg(target_arch = "wasm32")]
const WIRE_ERROR_NAME: &[u8] = b"WireError\0";

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
unsafe extern "C" fn kobako_rpc_call(
    mrb: *mut sys::mrb_state,
    _self_: sys::mrb_value,
) -> sys::mrb_value {
    #[cfg(target_arch = "wasm32")]
    {
        use crate::codec::Value;
        use crate::envelope::Target;
        use crate::rpc_client::invoke_rpc;

        // Unpack 4 required positional args: target, method, args_ary, kwargs_hash
        let mut target_val = sys::mrb_value::zeroed();
        let mut method_val = sys::mrb_value::zeroed();
        let mut args_ary = sys::mrb_value::zeroed();
        let mut kwargs_hash = sys::mrb_value::zeroed();

        sys::mrb_get_args(
            mrb,
            b"oooo\0".as_ptr() as *const core::ffi::c_char,
            &mut target_val as *mut sys::mrb_value,
            &mut method_val as *mut sys::mrb_value,
            &mut args_ary as *mut sys::mrb_value,
            &mut kwargs_hash as *mut sys::mrb_value,
        );

        // Decode target: String path or Kobako::Handle instance.
        let target = {
            let classname_ptr = sys::mrb_obj_classname(mrb, target_val);
            let classname = if classname_ptr.is_null() {
                ""
            } else {
                core::ffi::CStr::from_ptr(classname_ptr).to_str().unwrap_or("")
            };
            match classname {
                "Kobako::Handle" => {
                    // Handle target: extract id from @__kobako_id__ ivar.
                    let id_sym = sys::mrb_intern_cstr(
                        mrb, HANDLE_ID_IVAR.as_ptr() as *const core::ffi::c_char
                    );
                    let id_val = sys::mrb_iv_get(mrb, target_val, id_sym);
                    let id_str_val = sys::mrb_funcall(
                        mrb, id_val, b"to_s\0".as_ptr() as *const core::ffi::c_char, 0
                    );
                    let ptr = sys::mrb_str_to_cstr(mrb, id_str_val);
                    let id: u32 = if ptr.is_null() { 0 } else {
                        core::ffi::CStr::from_ptr(ptr).to_str().unwrap_or("0").parse().unwrap_or(0)
                    };
                    Target::Handle(id)
                }
                _ => {
                    let ptr = sys::mrb_str_to_cstr(mrb, target_val);
                    let s = if ptr.is_null() { "" } else {
                        core::ffi::CStr::from_ptr(ptr).to_str().unwrap_or("")
                    };
                    Target::Path(s.to_string())
                }
            }
        };

        // Decode method name string.
        let method_ptr = sys::mrb_str_to_cstr(mrb, method_val);
        let method_name = if method_ptr.is_null() {
            raise_wire_error(mrb, b"RPC method name is null\0");
        } else {
            core::ffi::CStr::from_ptr(method_ptr).to_str().unwrap_or("")
        };

        // Decode positional args from the Array.
        let args_len_val = sys::mrb_funcall(
            mrb, args_ary, b"length\0".as_ptr() as *const core::ffi::c_char, 0
        );
        let args_len_str = sys::mrb_str_to_cstr(mrb,
            sys::mrb_funcall(mrb, args_len_val, b"to_s\0".as_ptr() as *const core::ffi::c_char, 0)
        );
        let args_len: usize = if args_len_str.is_null() { 0 } else {
            core::ffi::CStr::from_ptr(args_len_str).to_str().unwrap_or("0").parse().unwrap_or(0)
        };
        let mut wire_args = Vec::with_capacity(args_len);
        for i in 0..args_len {
            let elem = sys::mrb_ary_entry(args_ary, i as i32);
            wire_args.push(mrb_value_to_wire_value(mrb, elem));
        }

        // Decode kwargs from the Hash.
        let mut wire_kwargs: Vec<(String, Value)> = Vec::new();
        // Check if kwargs_hash is a Hash by classname.
        {
            let kh_classname_ptr = sys::mrb_obj_classname(mrb, kwargs_hash);
            let kh_classname = if kh_classname_ptr.is_null() { "" } else {
                core::ffi::CStr::from_ptr(kh_classname_ptr).to_str().unwrap_or("")
            };
            if kh_classname == "Hash" {
                let keys_ary = sys::mrb_hash_keys(mrb, kwargs_hash);
                let keys_len_val = sys::mrb_funcall(
                    mrb, keys_ary, b"length\0".as_ptr() as *const core::ffi::c_char, 0
                );
                let keys_len_str = sys::mrb_str_to_cstr(mrb,
                    sys::mrb_funcall(mrb, keys_len_val, b"to_s\0".as_ptr() as *const core::ffi::c_char, 0)
                );
                let keys_len: usize = if keys_len_str.is_null() { 0 } else {
                    core::ffi::CStr::from_ptr(keys_len_str).to_str().unwrap_or("0").parse().unwrap_or(0)
                };
                for i in 0..keys_len {
                    let key_val = sys::mrb_ary_entry(keys_ary, i as i32);
                    let val = sys::mrb_hash_get(mrb, kwargs_hash, key_val);
                    // Key should be a Symbol; convert to String.
                    let key_str = mrb_sym_or_str_to_string(mrb, key_val);
                    wire_kwargs.push((key_str, mrb_value_to_wire_value(mrb, val)));
                }
            }
        }

        // Invoke RPC.
        match invoke_rpc(target, method_name, &wire_args, &wire_kwargs) {
            Ok(wire_val) => wire_value_to_mrb(mrb, wire_val),
            Err(crate::rpc_client::InvokeError::ServiceErr(ex)) => {
                // Raise ServiceError with the exception message.
                let kobako_mod = sys::mrb_define_module(
                    mrb, KOBAKO_NAME.as_ptr() as *const core::ffi::c_char
                );
                let svc_err_cls = sys::mrb_class_get_under(
                    mrb, kobako_mod, b"ServiceError\0".as_ptr() as *const core::ffi::c_char
                );
                let msg = std::ffi::CString::new(ex.message.as_str()).unwrap_or_default();
                sys::mrb_raise(mrb, svc_err_cls, msg.as_ptr());
            }
            Err(_) => {
                raise_wire_error(mrb, b"RPC wire error during invoke_rpc\0");
            }
        }
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
unsafe extern "C" fn rpc_method_missing(
    mrb: *mut sys::mrb_state,
    self_: sys::mrb_value,
) -> sys::mrb_value {
    #[cfg(target_arch = "wasm32")]
    {
        use crate::codec::Value;
        use crate::envelope::Target;
        use crate::rpc_client::invoke_rpc;

        // Unpack: method_name_sym + rest args.
        let mut method_sym: sys::mrb_sym = 0;
        let mut rest_ptr: *const sys::mrb_value = core::ptr::null();
        let mut rest_len: core::ffi::c_int = 0;

        sys::mrb_get_args(
            mrb,
            b"n*\0".as_ptr() as *const core::ffi::c_char,
            &mut method_sym as *mut sys::mrb_sym,
            &mut rest_ptr as *mut *const sys::mrb_value,
            &mut rest_len as *mut core::ffi::c_int,
        );

        // Get the target class name.
        // mrb_class_ptr(val) is the macro `((struct RClass*)(mrb_ptr(val)))`.
        // With MRB_WORDBOX_NO_INLINE_FLOAT + MRB_INT32 (our wasm32 config),
        // mrb_ptr(val) is `mrb_val_union(val).p` which for object-tagged values
        // is just the raw pointer stored in the lower bits of the word.
        // On wasm32 (32-bit address space), the mrb_value.w field IS the pointer
        // directly for object types. We implement the macro inline here to avoid
        // declaring mrb_class_ptr as an extern "C" fn (it is a C macro, not
        // a real function, so a Rust FFI declaration would produce an unresolved
        // wasm import).
        let class_ptr = self_.w as *mut sys::RClass;
        let class_name_ptr = sys::mrb_class_name(mrb, class_ptr);
        let target_str = if class_name_ptr.is_null() {
            raise_wire_error(mrb, b"RPC target class name is null\0");
        } else {
            core::ffi::CStr::from_ptr(class_name_ptr).to_str().unwrap_or("")
        };

        // Get the method name string from the symbol.
        let method_name_ptr = sys::mrb_sym_name(mrb, method_sym);
        let method_name = if method_name_ptr.is_null() {
            raise_wire_error(mrb, b"RPC method symbol name is null\0");
        } else {
            core::ffi::CStr::from_ptr(method_name_ptr).to_str().unwrap_or("")
        };

        // Build args and kwargs from rest_ptr[0..rest_len].
        let rest: &[sys::mrb_value] = if rest_len > 0 && !rest_ptr.is_null() {
            core::slice::from_raw_parts(rest_ptr, rest_len as usize)
        } else {
            &[]
        };

        let mut wire_args: Vec<Value> = Vec::new();
        let mut wire_kwargs: Vec<(String, Value)> = Vec::new();

        for (idx, &mrb_val) in rest.iter().enumerate() {
            let classname_ptr = sys::mrb_obj_classname(mrb, mrb_val);
            let classname = if classname_ptr.is_null() {
                ""
            } else {
                core::ffi::CStr::from_ptr(classname_ptr).to_str().unwrap_or("")
            };
            // If the last argument is a Hash, treat it as kwargs.
            if classname == "Hash" && idx == rest.len() - 1 {
                let keys_ary = sys::mrb_hash_keys(mrb, mrb_val);
                let keys_len_val = sys::mrb_funcall(
                    mrb, keys_ary, b"length\0".as_ptr() as *const core::ffi::c_char, 0
                );
                let keys_len_str = sys::mrb_str_to_cstr(mrb,
                    sys::mrb_funcall(mrb, keys_len_val, b"to_s\0".as_ptr() as *const core::ffi::c_char, 0)
                );
                let keys_len: usize = if keys_len_str.is_null() { 0 } else {
                    core::ffi::CStr::from_ptr(keys_len_str).to_str().unwrap_or("0").parse().unwrap_or(0)
                };
                for i in 0..keys_len {
                    let key = sys::mrb_ary_entry(keys_ary, i as i32);
                    let val = sys::mrb_hash_get(mrb, mrb_val, key);
                    let key_str = mrb_sym_or_str_to_string(mrb, key);
                    wire_kwargs.push((key_str, mrb_value_to_wire_value(mrb, val)));
                }
            } else {
                wire_args.push(mrb_value_to_wire_value(mrb, mrb_val));
            }
        }

        let target = Target::Path(target_str.to_string());

        match invoke_rpc(target, method_name, &wire_args, &wire_kwargs) {
            Ok(wire_val) => wire_value_to_mrb(mrb, wire_val),
            Err(crate::rpc_client::InvokeError::ServiceErr(ex)) => {
                let kobako_mod = sys::mrb_define_module(
                    mrb, KOBAKO_NAME.as_ptr() as *const core::ffi::c_char
                );
                let svc_err_cls = sys::mrb_class_get_under(
                    mrb, kobako_mod, b"ServiceError\0".as_ptr() as *const core::ffi::c_char
                );
                let msg = std::ffi::CString::new(ex.message.as_str()).unwrap_or_default();
                sys::mrb_raise(mrb, svc_err_cls, msg.as_ptr());
            }
            Err(_) => {
                raise_wire_error(mrb, b"RPC wire error\0");
            }
        }
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
unsafe extern "C" fn handle_initialize(
    mrb: *mut sys::mrb_state,
    self_: sys::mrb_value,
) -> sys::mrb_value {
    #[cfg(target_arch = "wasm32")]
    {
        let mut id_val = sys::mrb_value::zeroed();
        sys::mrb_get_args(
            mrb,
            b"o\0".as_ptr() as *const core::ffi::c_char,
            &mut id_val as *mut sys::mrb_value,
        );
        let sym = sys::mrb_intern_cstr(mrb, HANDLE_ID_IVAR.as_ptr() as *const core::ffi::c_char);
        sys::mrb_iv_set(mrb, self_, sym, id_val);
    }
    sys::mrb_value::zeroed()
}

/// `Kobako::Handle#method_missing(name, *args)` C bridge.
///
/// Forwards every method call on a Handle instance to the host via
/// `Kobako.__rpc_call__(id, method_name, args, kwargs)` with the Handle id
/// as an integer target (SPEC.md §B-17 — Handle chaining).
#[allow(unused_variables)]
unsafe extern "C" fn handle_method_missing(
    mrb: *mut sys::mrb_state,
    self_: sys::mrb_value,
) -> sys::mrb_value {
    #[cfg(target_arch = "wasm32")]
    {
        use crate::codec::Value;
        use crate::envelope::Target;
        use crate::rpc_client::invoke_rpc;

        // Unpack: method_name_sym + rest args.
        let mut method_sym: sys::mrb_sym = 0;
        let mut rest_ptr: *const sys::mrb_value = core::ptr::null();
        let mut rest_len: core::ffi::c_int = 0;

        sys::mrb_get_args(
            mrb,
            b"n*\0".as_ptr() as *const core::ffi::c_char,
            &mut method_sym as *mut sys::mrb_sym,
            &mut rest_ptr as *mut *const sys::mrb_value,
            &mut rest_len as *mut core::ffi::c_int,
        );

        // Retrieve the Handle id from @__kobako_id__.
        let id_sym = sys::mrb_intern_cstr(mrb, HANDLE_ID_IVAR.as_ptr() as *const core::ffi::c_char);
        let id_val = sys::mrb_iv_get(mrb, self_, id_sym);
        // Extract the integer id via to_s + parse.
        let id_str_val = sys::mrb_funcall(
            mrb, id_val, b"to_s\0".as_ptr() as *const core::ffi::c_char, 0
        );
        let id_ptr = sys::mrb_str_to_cstr(mrb, id_str_val);
        let handle_id: u32 = if id_ptr.is_null() { 0 } else {
            core::ffi::CStr::from_ptr(id_ptr).to_str().unwrap_or("0").parse().unwrap_or(0)
        };

        // Get the method name string from the symbol.
        let method_name_ptr = sys::mrb_sym_name(mrb, method_sym);
        let method_name = if method_name_ptr.is_null() {
            raise_wire_error(mrb, b"Handle method symbol name is null\0");
        } else {
            core::ffi::CStr::from_ptr(method_name_ptr).to_str().unwrap_or("")
        };

        // Build args and kwargs from rest_ptr[0..rest_len].
        let rest: &[sys::mrb_value] = if rest_len > 0 && !rest_ptr.is_null() {
            core::slice::from_raw_parts(rest_ptr, rest_len as usize)
        } else {
            &[]
        };

        let mut wire_args: Vec<Value> = Vec::new();
        let mut wire_kwargs: Vec<(String, Value)> = Vec::new();

        for (idx, &mrb_val) in rest.iter().enumerate() {
            let classname_ptr = sys::mrb_obj_classname(mrb, mrb_val);
            let classname = if classname_ptr.is_null() {
                ""
            } else {
                core::ffi::CStr::from_ptr(classname_ptr).to_str().unwrap_or("")
            };
            if classname == "Hash" && idx == rest.len() - 1 {
                let keys_ary = sys::mrb_hash_keys(mrb, mrb_val);
                let keys_len_val = sys::mrb_funcall(
                    mrb, keys_ary, b"length\0".as_ptr() as *const core::ffi::c_char, 0
                );
                let keys_len_str = sys::mrb_str_to_cstr(mrb,
                    sys::mrb_funcall(mrb, keys_len_val, b"to_s\0".as_ptr() as *const core::ffi::c_char, 0)
                );
                let keys_len: usize = if keys_len_str.is_null() { 0 } else {
                    core::ffi::CStr::from_ptr(keys_len_str).to_str().unwrap_or("0").parse().unwrap_or(0)
                };
                for i in 0..keys_len {
                    let key = sys::mrb_ary_entry(keys_ary, i as i32);
                    let val = sys::mrb_hash_get(mrb, mrb_val, key);
                    let key_str = mrb_sym_or_str_to_string(mrb, key);
                    wire_kwargs.push((key_str, mrb_value_to_wire_value(mrb, val)));
                }
            } else {
                wire_args.push(mrb_value_to_wire_value(mrb, mrb_val));
            }
        }

        let target = Target::Handle(handle_id);

        match invoke_rpc(target, method_name, &wire_args, &wire_kwargs) {
            Ok(wire_val) => wire_value_to_mrb(mrb, wire_val),
            Err(crate::rpc_client::InvokeError::ServiceErr(ex)) => {
                let kobako_mod = sys::mrb_define_module(
                    mrb, KOBAKO_NAME.as_ptr() as *const core::ffi::c_char
                );
                let svc_err_cls = sys::mrb_class_get_under(
                    mrb, kobako_mod, b"ServiceError\0".as_ptr() as *const core::ffi::c_char
                );
                let msg = std::ffi::CString::new(ex.message.as_str()).unwrap_or_default();
                sys::mrb_raise(mrb, svc_err_cls, msg.as_ptr());
            }
            Err(_) => {
                raise_wire_error(mrb, b"RPC wire error (Handle dispatch)\0");
            }
        }
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
unsafe extern "C" fn rpc_respond_to_missing(
    _mrb: *mut sys::mrb_state,
    _self_: sys::mrb_value,
) -> sys::mrb_value {
    #[cfg(target_arch = "wasm32")]
    {
        mrb_true_value()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        sys::mrb_value::zeroed()
    }
}

// --------------------------------------------------------------------
// Value conversion helpers (wasm32-only).
// --------------------------------------------------------------------

/// Convert a kobako wire `Value` to an `mrb_value`. Used to box the RPC
/// response back into the mruby VM after `invoke_rpc` succeeds.
///
/// Covers the wire types the journey tests exercise (SPEC.md §Type Mapping):
/// Nil, Bool, Int, Float, Str, Handle (as Integer). UInt, Bin, Array, Map,
/// ErrEnv are not required by the J-01..J-05 journeys; Array/Map support is
/// a follow-up item.
#[cfg(target_arch = "wasm32")]
unsafe fn wire_value_to_mrb(mrb: *mut sys::mrb_state, val: crate::codec::Value) -> sys::mrb_value {
    use crate::codec::Value;
    match val {
        Value::Nil => mrb_nil_value(),
        Value::Bool(b) => if b { mrb_true_value() } else { mrb_false_value() },
        Value::Int(n) => {
            // mrb_boxing_int_value(mrb, n) is the proper API for constructing
            // a boxed integer without hand-coding the bit layout.
            // mrb_int on wasm32 is 32-bit (MRB_INT32 config); clamp to i32.
            let n32 = n.clamp(i32::MIN as i64, i32::MAX as i64) as i32;
            sys::mrb_boxing_int_value(mrb, n32)
        }
        Value::UInt(n) => {
            // UInt that fits in i32 → Int path; otherwise clamp.
            let n32 = n.min(i32::MAX as u64) as i32;
            sys::mrb_boxing_int_value(mrb, n32)
        }
        Value::Float(f) => {
            // mrb_word_boxing_float_value allocates a boxed float object on
            // the mruby heap (required for MRB_WORDBOX_NO_INLINE_FLOAT on wasm32).
            sys::mrb_word_boxing_float_value(mrb, f)
        }
        Value::Str(s) => {
            match std::ffi::CString::new(s.as_str()) {
                Ok(cs) => sys::mrb_str_new_cstr(mrb, cs.as_ptr()),
                Err(_) => {
                    // String contains NUL bytes; use mrb_str_new with explicit len.
                    // mrb_int on wasm32 is 32-bit; len fits as i32 for any sane string.
                    sys::mrb_str_new(mrb, s.as_ptr() as *const core::ffi::c_char, s.len() as i32)
                }
            }
        }
        Value::Handle(id) => {
            // Return a Kobako::Handle instance carrying the id. Instance-level
            // method_missing on Kobako::Handle routes subsequent calls to the
            // host via Kobako.__rpc_call__ with Target::Handle(id) (SPEC §B-17).
            let kobako_mod = sys::mrb_define_module(
                mrb, KOBAKO_NAME.as_ptr() as *const core::ffi::c_char
            );
            let handle_class = sys::mrb_class_get_under(
                mrb, kobako_mod, HANDLE_NAME.as_ptr() as *const core::ffi::c_char
            );
            // Build the constructor argument: mrb_int id (mrb_boxing_int_value).
            let id_val = sys::mrb_boxing_int_value(mrb, id as i32);
            // mrb_obj_new calls Kobako::Handle.new(id) which triggers
            // handle_initialize to store @__kobako_id__.
            sys::mrb_obj_new(mrb, handle_class, 1, &id_val as *const sys::mrb_value)
        }
        Value::Bin(bytes) => {
            // Binary data as mruby String (binary-transparent).
            // mrb_int on wasm32 is 32-bit; len fits as i32 for any sane buffer.
            sys::mrb_str_new(
                mrb,
                bytes.as_ptr() as *const core::ffi::c_char,
                bytes.len() as i32,
            )
        }
        Value::Array(_) | Value::Map(_) | Value::ErrEnv(_) => {
            // Complex types: return nil. Full Array/Hash support requires
            // mrb_ary_new + iteration; not needed for J-01..J-05 journeys.
            mrb_nil_value()
        }
    }
}

/// Convert an `mrb_value` to a kobako wire `Value`. Used when building
/// args/kwargs for `invoke_rpc` from mruby-side values.
#[cfg(target_arch = "wasm32")]
unsafe fn mrb_value_to_wire_value(mrb: *mut sys::mrb_state, val: sys::mrb_value) -> crate::codec::Value {
    use crate::codec::Value;

    let classname_ptr = sys::mrb_obj_classname(mrb, val);
    let classname = if classname_ptr.is_null() {
        ""
    } else {
        core::ffi::CStr::from_ptr(classname_ptr).to_str().unwrap_or("")
    };

    match classname {
        "NilClass" => Value::Nil,
        "TrueClass" => Value::Bool(true),
        "FalseClass" => Value::Bool(false),
        "Integer" => {
            let s_val = sys::mrb_funcall(
                mrb, val, b"to_s\0".as_ptr() as *const core::ffi::c_char, 0
            );
            let ptr = sys::mrb_str_to_cstr(mrb, s_val);
            if ptr.is_null() {
                Value::Int(0)
            } else {
                let s = core::ffi::CStr::from_ptr(ptr).to_str().unwrap_or("0");
                Value::Int(s.parse::<i64>().unwrap_or(0))
            }
        }
        "Float" => {
            let s_val = sys::mrb_funcall(
                mrb, val, b"to_s\0".as_ptr() as *const core::ffi::c_char, 0
            );
            let ptr = sys::mrb_str_to_cstr(mrb, s_val);
            if ptr.is_null() {
                Value::Float(0.0)
            } else {
                let s = core::ffi::CStr::from_ptr(ptr).to_str().unwrap_or("0.0");
                Value::Float(s.parse::<f64>().unwrap_or(0.0))
            }
        }
        "String" => {
            let ptr = sys::mrb_str_to_cstr(mrb, val);
            if ptr.is_null() {
                Value::Str(String::new())
            } else {
                let s = core::ffi::CStr::from_ptr(ptr).to_str().unwrap_or("").to_string();
                Value::Str(s)
            }
        }
        "Symbol" => {
            // Symbols: convert to string via mrb_sym_str or .to_s.
            let str_val = sys::mrb_funcall(
                mrb, val, b"to_s\0".as_ptr() as *const core::ffi::c_char, 0
            );
            let ptr = sys::mrb_str_to_cstr(mrb, str_val);
            if ptr.is_null() {
                Value::Str(String::new())
            } else {
                Value::Str(core::ffi::CStr::from_ptr(ptr).to_str().unwrap_or("").to_string())
            }
        }
        _ => {
            // Unknown type: inspect string fallback.
            let insp_val = sys::mrb_funcall(
                mrb, val, b"to_s\0".as_ptr() as *const core::ffi::c_char, 0
            );
            let ptr = sys::mrb_str_to_cstr(mrb, insp_val);
            if ptr.is_null() {
                Value::Str(String::new())
            } else {
                Value::Str(core::ffi::CStr::from_ptr(ptr).to_str().unwrap_or("").to_string())
            }
        }
    }
}

/// Convert a Symbol or String `mrb_value` to a Rust String. Used for
/// Hash key extraction in kwargs decoding.
#[cfg(target_arch = "wasm32")]
unsafe fn mrb_sym_or_str_to_string(mrb: *mut sys::mrb_state, val: sys::mrb_value) -> String {
    let classname_ptr = sys::mrb_obj_classname(mrb, val);
    let classname = if classname_ptr.is_null() {
        ""
    } else {
        core::ffi::CStr::from_ptr(classname_ptr).to_str().unwrap_or("")
    };
    let str_val = if classname == "Symbol" {
        sys::mrb_funcall(mrb, val, b"to_s\0".as_ptr() as *const core::ffi::c_char, 0)
    } else {
        val
    };
    let ptr = sys::mrb_str_to_cstr(mrb, str_val);
    if ptr.is_null() {
        String::new()
    } else {
        core::ffi::CStr::from_ptr(ptr).to_str().unwrap_or("").to_string()
    }
}

/// Construct an mruby `nil` value.
///
/// In mruby's word-boxing ABI on wasm32, `mrb_value.w = 0` is nil
/// (MRB_Qnil = 0). With our corrected `mrb_value { w: u32 }` layout,
/// `mrb_value::zeroed()` gives the right representation.
#[cfg(target_arch = "wasm32")]
fn mrb_nil_value() -> sys::mrb_value {
    sys::mrb_value { w: 0 } // MRB_Qnil = 0
}

/// Construct an mruby `true` value (MRB_Qtrue = 12).
#[cfg(target_arch = "wasm32")]
fn mrb_true_value() -> sys::mrb_value {
    sys::mrb_value { w: 12 } // MRB_Qtrue = 12
}

/// Construct an mruby `false` value (MRB_Qfalse = 4).
#[cfg(target_arch = "wasm32")]
fn mrb_false_value() -> sys::mrb_value {
    sys::mrb_value { w: 4 } // MRB_Qfalse = 4
}

// --------------------------------------------------------------------
// `Kobako::WireError` raise helper.
// --------------------------------------------------------------------
//
// Resolves `Kobako::WireError` (defined host-side; the guest sees it
// because the host class registry seeds it during sandbox start, see
// SPEC §Error attribution) and raises with the supplied null-
// terminated C string message. Diverges (`-> !`) — `mrb_raise` does
// not return.

#[cfg(target_arch = "wasm32")]
unsafe fn raise_wire_error(mrb: *mut sys::mrb_state, msg: &[u8]) -> ! {
    let kobako_mod = sys::mrb_define_module(
        mrb,
        KOBAKO_NAME.as_ptr() as *const core::ffi::c_char,
    );
    let cls = sys::mrb_class_get_under(
        mrb,
        kobako_mod,
        WIRE_ERROR_NAME.as_ptr() as *const core::ffi::c_char,
    );
    sys::mrb_raise(
        mrb,
        cls,
        msg.as_ptr() as *const core::ffi::c_char,
    );
}

// --------------------------------------------------------------------
// Tests — host target.
// --------------------------------------------------------------------
//
// On host the FFI calls are absent (`#[cfg(target_arch = "wasm32")]`).
// What we *can* test cheaply is that the function items compile with
// the documented signatures and that the C-string constants are well
// formed (NUL-terminated, ASCII). C API signature regressions surface
// as compile errors in `mruby_sys.rs` — we don't need duplicate runtime
// asserts.

#[cfg(test)]
mod tests {
    use super::*;

    fn is_ascii_nul_terminated(s: &[u8]) -> bool {
        !s.is_empty() && s[s.len() - 1] == 0 && s[..s.len() - 1].iter().all(|b| b.is_ascii() && *b != 0)
    }

    #[test]
    fn c_string_constants_are_well_formed() {
        // mruby C API takes `const char*`. Each constant must be
        // ASCII, contain no embedded NUL, and end in NUL.
        for (label, s) in &[
            ("KOBAKO_NAME", KOBAKO_NAME),
            ("RPC_NAME", RPC_NAME),
            ("HANDLE_NAME", HANDLE_NAME),
            ("RPC_CALL_NAME", RPC_CALL_NAME),
            ("METHOD_MISSING_NAME", METHOD_MISSING_NAME),
            ("RESPOND_TO_MISSING_NAME", RESPOND_TO_MISSING_NAME),
            ("INITIALIZE_NAME", INITIALIZE_NAME),
        ] {
            assert!(
                is_ascii_nul_terminated(s),
                "{label} must be ASCII + NUL-terminated, got {s:?}"
            );
        }
    }

    #[test]
    fn ruby_names_match_boot_contract() {
        // The boot contract fixes these names exactly.
        assert_eq!(&KOBAKO_NAME[..KOBAKO_NAME.len() - 1], b"Kobako");
        assert_eq!(&RPC_NAME[..RPC_NAME.len() - 1], b"RPC");
        assert_eq!(&HANDLE_NAME[..HANDLE_NAME.len() - 1], b"Handle");
        assert_eq!(&RPC_CALL_NAME[..RPC_CALL_NAME.len() - 1], b"__rpc_call__");
        assert_eq!(
            &METHOD_MISSING_NAME[..METHOD_MISSING_NAME.len() - 1],
            b"method_missing"
        );
    }

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
