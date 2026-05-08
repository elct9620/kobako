//! Guest Binary boot — Rust-side mruby C API registrations.
//!
//! This module replaces the previous `boot.rb` + `include_str!`
//! mechanism with the **mruby C API path** REFERENCE Ch.5 §Boot Script
//! 預載 (lines 944–985) specifies. No Ruby text is loaded into the
//! mruby VM at boot time; instead, the three foundational entities are
//! registered directly via C API calls:
//!
//!   1. `Kobako` module — `mrb_define_module(mrb, "Kobako")`
//!      (REFERENCE line 948).
//!   2. `Kobako::RPC` base class — `mrb_define_class_under(mrb,
//!      kobako_mod, "RPC", mrb->object_class)`. Each Service Member
//!      (e.g. `MyService::KV`) is, at runtime, a *subclass* of
//!      `Kobako::RPC` created by the Frame 1 preamble — they inherit
//!      the singleton-class `method_missing` installed here
//!      (REFERENCE lines 950–957).
//!   3. `Kobako.__rpc_call__(target, method, args, kwargs)` —
//!      `mrb_define_module_function(mrb, kobako_mod, "__rpc_call__",
//!      c_fn, MRB_ARGS_REQ(4))` (REFERENCE line 959). The four-arg
//!      module function is the single dispatch entry point shared by
//!      both `Kobako::RPC` subclasses (path target) and `Kobako::Handle`
//!      instances (handle target — wire ext 0x01).
//!
//! REFERENCE Ch.5 line 946 explicitly forbids `mrb_load_string` for
//! the boot/preload phase — every entity is defined via C API. Line
//! 977 forbids hand-rolled `mrb_value` bit construction; this file
//! never inspects or constructs `mrb_value` payloads — it forwards
//! them through the FFI shims in `mruby_sys.rs`.
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
//! moved per REFERENCE Ch.5 §Boot Script 三職責 (lines 989–1033):
//!
//!   * "State init / capture $stdout/$stderr" — REFERENCE line 1027
//!     pins stdout/stderr as **user-observable channels** delivered by
//!     wasi fds 1/2. The host side reads them through `Sandbox#stdout`
//!     / `Sandbox#stderr`. No mruby-side capture is needed.
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
/// `b"__rpc_call__\0"`.
const RPC_CALL_NAME: &[u8] = b"__rpc_call__\0";
/// `b"method_missing\0"`.
const METHOD_MISSING_NAME: &[u8] = b"method_missing\0";
/// `b"respond_to_missing?\0"`.
const RESPOND_TO_MISSING_NAME: &[u8] = b"respond_to_missing?\0";

// --------------------------------------------------------------------
// Public entry point.
// --------------------------------------------------------------------

/// Register `Kobako` module, `Kobako::RPC` base class, and
/// `Kobako.__rpc_call__` module function on the given mruby state.
///
/// REFERENCE Ch.5 §Boot Script 預載 (lines 946–977) is the normative
/// spec — this function is the Rust mirror of every step listed there.
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
        // (1) `mrb_define_module(mrb, "Kobako")` — REFERENCE line 948.
        let kobako_mod = sys::mrb_define_module(
            mrb,
            KOBAKO_NAME.as_ptr() as *const core::ffi::c_char,
        );

        // (2) `Kobako::RPC` base class — REFERENCE line 950.
        //
        // The super-class is `mrb->object_class`. Per REFERENCE the
        // standard idiom is `mrb_define_class_under(mrb, kobako_mod,
        // "RPC", mrb->object_class)`. We pass `core::ptr::null_mut()`
        // for `super_` here: mruby's `mrb_define_class_under` accepts
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
        //     on `Kobako::RPC` — REFERENCE lines 952–953.
        //
        // `mrb_define_singleton_method` takes the *object* whose
        // singleton-class receives the method. For class-level
        // `method_missing` the object is the class itself, cast to
        // `RObject*`. Subclasses inherit through metaclass-chain
        // dispatch (REFERENCE line 954).
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
        //     args — REFERENCE line 959.
        sys::mrb_define_module_function(
            mrb,
            kobako_mod,
            RPC_CALL_NAME.as_ptr() as *const core::ffi::c_char,
            kobako_rpc_call,
            sys::mrb_args_req(4),
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
const NOT_WIRED_MSG: &[u8] =
    b"Kobako: __rpc_call__ body lands with item #16 (Sandbox#run wiring)\0";

#[cfg(target_arch = "wasm32")]
const WIRE_ERROR_NAME: &[u8] = b"WireError\0";

/// `Kobako.__rpc_call__(target, method, args, kwargs)` C bridge.
///
/// Item #16 fills the body — argument unpack via `mrb_get_args`, encode
/// to `crate::envelope::Request`, dispatch through
/// `crate::rpc_client::invoke_rpc`, decode `Response` back to
/// `mrb_value`. Today this raises `Kobako::WireError` with a
/// "not yet wired" message.
#[allow(unused_variables)]
unsafe extern "C" fn kobako_rpc_call(
    mrb: *mut sys::mrb_state,
    self_: sys::mrb_value,
) -> sys::mrb_value {
    #[cfg(target_arch = "wasm32")]
    {
        raise_wire_error(mrb, NOT_WIRED_MSG);
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        sys::mrb_value::zeroed()
    }
}

/// `Kobako::RPC.method_missing(name, *args, &block)` C bridge.
///
/// REFERENCE Ch.5 lines 952, 955–956 specify the contract: the
/// receiver `self` is the calling class object (e.g. `MyService::KV`),
/// the method symbol becomes the wire-level `method` field, and
/// trailing Hash args are extracted as kwargs. Item #16 fills the
/// body using the same dispatch pipeline as `kobako_rpc_call`.
#[allow(unused_variables)]
unsafe extern "C" fn rpc_method_missing(
    mrb: *mut sys::mrb_state,
    self_: sys::mrb_value,
) -> sys::mrb_value {
    #[cfg(target_arch = "wasm32")]
    {
        raise_wire_error(mrb, NOT_WIRED_MSG);
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        sys::mrb_value::zeroed()
    }
}

/// `Kobako::RPC.respond_to_missing?(name, include_private)` C bridge.
///
/// Always returns `true` per REFERENCE line 953. Item #16 wires the
/// body using `mrb_true_value()` from the boxing-macro shim; today the
/// body raises `Kobako::WireError` to keep behaviour uniformly
/// "not yet wired" until the boxing shims are bound. Calls in user
/// code that depend on this method's truthy return are out of scope
/// for this item.
#[allow(unused_variables)]
unsafe extern "C" fn rpc_respond_to_missing(
    mrb: *mut sys::mrb_state,
    self_: sys::mrb_value,
) -> sys::mrb_value {
    #[cfg(target_arch = "wasm32")]
    {
        raise_wire_error(mrb, NOT_WIRED_MSG);
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        sys::mrb_value::zeroed()
    }
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
// formed (NUL-terminated, ASCII). REFERENCE alignment regressions
// surface as compile errors in `mruby_sys.rs` — we don't need
// duplicate runtime asserts.

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
            ("RPC_CALL_NAME", RPC_CALL_NAME),
            ("METHOD_MISSING_NAME", METHOD_MISSING_NAME),
            ("RESPOND_TO_MISSING_NAME", RESPOND_TO_MISSING_NAME),
        ] {
            assert!(
                is_ascii_nul_terminated(s),
                "{label} must be ASCII + NUL-terminated, got {s:?}"
            );
        }
    }

    #[test]
    fn ruby_names_match_reference_ch5() {
        // REFERENCE line 946–959 fixes these names exactly.
        assert_eq!(&KOBAKO_NAME[..KOBAKO_NAME.len() - 1], b"Kobako");
        assert_eq!(&RPC_NAME[..RPC_NAME.len() - 1], b"RPC");
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
    }
}
