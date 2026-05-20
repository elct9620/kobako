//! Boot helpers shared by `__kobako_eval` and `__kobako_run`.
//!
//! Both entry points open a fresh mruby VM, install the Kobako runtime,
//! materialise the Frame 1 preamble namespaces, and replay any
//! preloaded Frame 3 snippets before running the entry-specific body.
//! When any of those steps fails, the failure surfaces as a Panic with
//! `origin = "sandbox"` and `class = "Kobako::BootError"` — this module
//! centralises both the orchestration and the Panic-construction
//! shape.
//!
//! Snippet replay (docs/behavior.md B-32) compiles each snippet under a
//! `(snippet:Name)` filename so any uncaught exception's backtrace
//! attributes back to the originating `#preload` call. Replay failures
//! are always sandbox-origin even when the raised class would otherwise
//! map to "service" — preloaded snippets are sandbox code.

#[cfg(target_arch = "wasm32")]
use crate::cstr;
#[cfg(target_arch = "wasm32")]
use crate::kobako::{InstallGroupsError, Kobako};
#[cfg(target_arch = "wasm32")]
use crate::mruby::ccontext::Ccontext;
#[cfg(target_arch = "wasm32")]
use crate::mruby::Mrb;
use crate::outcome::Panic;

/// Build a Panic envelope carrying the kobako boot defaults
/// (`origin = "sandbox"`, `class = "Kobako::BootError"`, empty
/// backtrace, no details). The exclusive constructor for the
/// `Kobako::BootError` panic shape — every boot-time failure should
/// pass through here so the host-visible attribution stays uniform.
pub(super) fn boot_panic(message: impl Into<String>) -> Panic {
    Panic {
        origin: "sandbox".into(),
        class: "Kobako::BootError".into(),
        message: message.into(),
        backtrace: Vec::new(),
        details: None,
    }
}

/// Decide which Panic `origin` field a given mruby exception class
/// should produce. Mirrors the docs/behavior.md attribution rules —
/// a `Kobako::ServiceError` raised from a Service capability lands on
/// `"service"`; everything else lands on `"sandbox"`. Pure string
/// inspection — host-buildable for unit tests.
pub(super) fn origin_for_class(class_name: &str) -> &'static str {
    if class_name.contains("ServiceError") {
        "service"
    } else {
        "sandbox"
    }
}

/// Read Frame 1 from stdin and decode it into the Group / Member list.
/// Either step failing surfaces as a [`boot_panic`].
#[cfg(target_arch = "wasm32")]
pub(super) fn read_preamble() -> Result<Vec<(String, Vec<String>)>, Panic> {
    let bytes =
        super::frames::read_frame().ok_or_else(|| boot_panic("failed to read preamble frame"))?;
    super::frames::decode_preamble(&bytes)
        .ok_or_else(|| boot_panic("failed to decode preamble msgpack"))
}

/// Read Frame 3 from stdin and decode it into the snippet list.
#[cfg(target_arch = "wasm32")]
pub(super) fn read_snippets() -> Result<Vec<super::frames::Snippet>, Panic> {
    let bytes =
        super::frames::read_frame().ok_or_else(|| boot_panic("failed to read snippets frame"))?;
    super::frames::decode_snippets(&bytes)
        .ok_or_else(|| boot_panic("failed to decode snippets msgpack"))
}

/// Open an mruby VM, install the Kobako runtime, then materialise the
/// Group / Member proxy classes from `preamble`. Returns the live
/// [`Mrb`] + [`Kobako`] pair so the entry-specific body can keep
/// driving the same VM.
#[cfg(target_arch = "wasm32")]
pub(super) fn open_with_preamble(
    preamble: &[(String, Vec<String>)],
) -> Result<(Mrb, Kobako), Panic> {
    let mrb = Mrb::open().map_err(|_| boot_panic("mrb_open returned NULL"))?;
    let kobako = Kobako::install(&mrb);
    kobako.install_groups(preamble).map_err(|err| match err {
        InstallGroupsError::NulInGroupName => boot_panic("group name contains NUL byte"),
        InstallGroupsError::NulInMemberName => boot_panic("member name contains NUL byte"),
    })?;
    Ok((mrb, kobako))
}

/// Replay every snippet in `snippets` against `mrb` in insertion order
/// so any uncaught exception's backtrace attributes back to the
/// originating `#preload` call (docs/behavior.md B-32). Source entries
/// load via a fresh ccontext under `(snippet:Name)` filenames; bytecode
/// entries load through `kobako_load_bytecode` (the filename, when
/// present, is baked into their RITE `debug_info` section). The first
/// snippet that raises wins: the resulting Panic carries that snippet's
/// class / message / backtrace and is forced to sandbox origin even
/// when [`origin_for_class`] would have chosen `"service"`. Bytecode
/// entries whose load returned a structural-failure code (E-37 / E-38)
/// additionally override the panic class to `Kobako::BytecodeError`;
/// a successful load that then raised at top level (E-36) keeps the
/// natural mruby class.
#[cfg(target_arch = "wasm32")]
pub(super) fn replay_snippets(
    mrb: &Mrb,
    kobako: &Kobako,
    snippets: &[super::frames::Snippet],
) -> Result<(), Panic> {
    for entry in snippets {
        let bytecode_structural_failure = match entry {
            super::frames::Snippet::Source { name, body } => {
                load_source_snippet(mrb, name, body)?;
                false
            }
            super::frames::Snippet::Bytecode { body } => load_bytecode_snippet(mrb, body) != 0,
        };
        if let Some(mut panic) = take_pending_panic(mrb, kobako) {
            // Replay-time failures are always sandbox origin even when
            // the class would normally map to service.
            panic.origin = "sandbox".into();
            // Only structural failures (E-37 / E-38) attribute to
            // BytecodeError. A bytecode snippet that loaded cleanly
            // and then raised at top level is E-36, with the natural
            // mruby class preserved.
            if bytecode_structural_failure {
                panic.class = "Kobako::BytecodeError".into();
            }
            return Err(panic);
        }
    }
    Ok(())
}

/// Compile and execute a source snippet under a fresh ccontext whose
/// filename is `(snippet:Name)`. Surfaces ccontext allocation failure
/// as a [`boot_panic`]; any mruby compile / runtime fault is left in
/// `mrb->exc` for the shared `take_pending_panic` step.
#[cfg(target_arch = "wasm32")]
fn load_source_snippet(mrb: &Mrb, name: &str, body: &str) -> Result<(), Panic> {
    let filename = format!("(snippet:{})\0", name);
    let Some(cxt) = Ccontext::new(mrb, filename.as_ptr() as *const core::ffi::c_char) else {
        return Err(boot_panic("mrb_ccontext_new returned NULL"));
    };
    cxt.load_nstring(body.as_bytes());
    // `cxt` drops here — `mrb_ccontext_free` runs automatically.
    Ok(())
}

/// Execute a precompiled RITE bytecode blob via the
/// [`crate::mruby::sys::kobako_load_bytecode`] shim. The shim parses
/// the IREP and runs its top-level Proc. Returns 0 when the IREP
/// parsed (even if its top-level execution then raised — E-36) and
/// non-zero on a structural failure (RITE version mismatch / corrupt
/// body — E-37 / E-38). Either way, a pending exception is left in
/// `mrb->exc` for the shared `take_pending_panic` step; the caller
/// uses the return code to decide whether to promote the resulting
/// panic to `Kobako::BytecodeError`.
#[cfg(target_arch = "wasm32")]
fn load_bytecode_snippet(mrb: &Mrb, body: &[u8]) -> core::ffi::c_int {
    // SAFETY: `mrb` is live by the caller's contract on `&Mrb`;
    // `body` is a borrowed slice that outlives the synchronous call.
    unsafe {
        crate::mruby::sys::kobako_load_bytecode(
            mrb.as_ptr(),
            body.as_ptr() as *const core::ffi::c_void,
            body.len(),
        )
    }
}

/// If an mruby exception is pending on `mrb`, extract its class name,
/// message, and backtrace into a Panic envelope (with `origin` chosen
/// by [`origin_for_class`]). Returns `None` when no exception is
/// pending. Clears `mrb->exc` via [`Mrb::clear_exc`] before returning.
#[cfg(target_arch = "wasm32")]
pub(super) fn take_pending_panic(mrb: &Mrb, kobako: &Kobako) -> Option<Panic> {
    let exc_val = mrb.pending_exc();
    if exc_val.w == 0 {
        return None;
    }
    let class_name = {
        let cn = kobako.classname_of(exc_val);
        if cn.is_empty() {
            "RuntimeError".to_string()
        } else {
            cn.to_string()
        }
    };
    let message = {
        let msg_val = kobako.call_method(exc_val, cstr!("message"), &[]);
        let m = kobako.to_string_of(msg_val);
        if m.is_empty() {
            class_name.clone()
        } else {
            m
        }
    };
    let backtrace = kobako.extract_backtrace(exc_val);
    mrb.clear_exc();
    Some(Panic {
        origin: origin_for_class(&class_name).into(),
        class: class_name,
        message,
        backtrace,
        details: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boot_panic_carries_kobako_boot_defaults() {
        let p = boot_panic("failed to read preamble frame");
        assert_eq!(p.origin, "sandbox");
        assert_eq!(p.class, "Kobako::BootError");
        assert_eq!(p.message, "failed to read preamble frame");
        assert!(p.backtrace.is_empty());
        assert!(p.details.is_none());
    }

    #[test]
    fn origin_for_class_routes_service_errors_to_service() {
        assert_eq!(origin_for_class("Kobako::ServiceError"), "service");
        assert_eq!(
            origin_for_class("Kobako::ServiceError::Disconnected"),
            "service"
        );
    }

    #[test]
    fn origin_for_class_defaults_to_sandbox() {
        assert_eq!(origin_for_class("RuntimeError"), "sandbox");
        assert_eq!(origin_for_class("Kobako::RPC::WireError"), "sandbox");
        assert_eq!(origin_for_class("NoMethodError"), "sandbox");
    }
}
