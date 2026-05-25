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
use crate::kobako::{InstallGroupsError, Kobako};
#[cfg(target_arch = "wasm32")]
use crate::mruby::Ccontext;
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

/// Open an mruby VM, install it into [`super::mrb_slot::MRB`], wire
/// the Kobako runtime, then materialise the Group / Member proxy
/// classes from `preamble`. Returns the live [`Kobako`] handle so the
/// entry-specific body can keep driving the same VM through
/// [`super::mrb_slot::MRB`]. On any `Err`, the slot is cleared so the
/// caller's [`super::mrb_slot::MrbScope`] does not observe a half-set
/// state.
#[cfg(target_arch = "wasm32")]
pub(super) fn open_with_preamble(preamble: &[(String, Vec<String>)]) -> Result<Kobako, Panic> {
    let mrb = Mrb::open().map_err(|_| boot_panic("mrb_open returned NULL"))?;
    super::mrb_slot::MRB.install(mrb);

    let result: Result<Kobako, Panic> = (|| {
        let mrb = super::mrb_slot::MRB
            .as_ref()
            .expect("MRB just installed above");
        let kobako = Kobako::install(mrb);
        kobako.install_groups(preamble).map_err(|err| match err {
            InstallGroupsError::NulInGroupName => boot_panic("group name contains NUL byte"),
            InstallGroupsError::NulInMemberName => boot_panic("member name contains NUL byte"),
        })?;
        Ok(kobako)
    })();

    if result.is_err() {
        super::mrb_slot::MRB.clear();
    }
    result
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
        let load = match entry {
            super::frames::Snippet::Source { name, body } => {
                load_source_snippet(mrb, name, body)?;
                BytecodeLoad::Loaded
            }
            super::frames::Snippet::Bytecode { body } => load_bytecode_snippet(mrb, body),
        };
        if let Some(panic) = take_pending_panic(mrb, kobako) {
            return Err(reshape_replay_panic(panic, load));
        }
    }
    Ok(())
}

/// Apply the replay-specific reshape to a pending Panic. Replay-time
/// failures are always sandbox origin even when the class would
/// normally map to service. Structural failures (E-37 / E-38) further
/// override the class to `Kobako::BytecodeError`; a bytecode snippet
/// that loaded cleanly and then raised at top level is E-36 with the
/// natural mruby class preserved. Functional struct-update keeps the
/// reshape in one expression — no mid-life mutation of the panic
/// fields.
#[cfg(target_arch = "wasm32")]
fn reshape_replay_panic(panic: Panic, load: BytecodeLoad) -> Panic {
    let class = match load {
        BytecodeLoad::StructuralFailure => "Kobako::BytecodeError".into(),
        BytecodeLoad::Loaded => panic.class,
    };
    Panic {
        origin: "sandbox".into(),
        class,
        ..panic
    }
}

/// Outcome of a bytecode-form snippet load. Distinguishes the two
/// failure shapes the caller's class-override step needs to tell
/// apart: a successful parse (whose top-level execution may still have
/// raised — E-36, natural mruby class preserved) from a structural
/// failure on the RITE header / IREP body
/// ({docs/behavior.md E-37 / E-38}[link:../../../docs/behavior.md]),
/// which gets promoted to +Kobako::BytecodeError+.
#[cfg(target_arch = "wasm32")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BytecodeLoad {
    Loaded,
    StructuralFailure,
}

/// Compile and execute a source snippet under a fresh ccontext whose
/// filename is `(snippet:Name)`. Surfaces ccontext allocation failure
/// as a [`boot_panic`]; any mruby compile / runtime fault is left in
/// `mrb->exc` for the shared `take_pending_panic` step. A snippet
/// `name` carrying an interior NUL byte (wire violation) also fails
/// through [`boot_panic`] since `CString::new` rejects it.
#[cfg(target_arch = "wasm32")]
fn load_source_snippet(mrb: &Mrb, name: &str, body: &str) -> Result<(), Panic> {
    let filename = std::ffi::CString::new(format!("(snippet:{})", name))
        .map_err(|_| boot_panic("snippet name contains NUL byte"))?;
    let Some(cxt) = Ccontext::new(mrb, &filename) else {
        return Err(boot_panic("mrb_ccontext_new returned NULL"));
    };
    cxt.load_nstring(body.as_bytes());
    // `cxt` drops here — `mrb_ccontext_free` runs automatically.
    Ok(())
}

/// Execute a precompiled RITE bytecode blob via the
/// [`crate::mruby::sys::kobako_load_bytecode`] shim. The shim parses
/// the IREP and runs its top-level Proc. Returns
/// [`BytecodeLoad::Loaded`] when the IREP parsed (even if its top-
/// level execution then raised — E-36) and
/// [`BytecodeLoad::StructuralFailure`] when the RITE header / IREP
/// body failed structural validation (E-37 / E-38). Either way, a
/// pending exception is left in `mrb->exc` for the shared
/// `take_pending_panic` step. Folding the C return code into a typed
/// enum at the FFI boundary keeps the `c_int` from leaking into the
/// replay control flow.
#[cfg(target_arch = "wasm32")]
fn load_bytecode_snippet(mrb: &Mrb, body: &[u8]) -> BytecodeLoad {
    if mrb.load_bytecode(body) == 0 {
        BytecodeLoad::Loaded
    } else {
        BytecodeLoad::StructuralFailure
    }
}

/// If an mruby exception is pending on `mrb`, extract its class name,
/// message, and backtrace into a Panic envelope (with `origin` chosen
/// by [`origin_for_class`]). Returns `None` when no exception is
/// pending. Clears `mrb->exc` via [`Mrb::clear_exc`] before returning.
#[cfg(target_arch = "wasm32")]
pub(super) fn take_pending_panic(mrb: &Mrb, kobako: &Kobako) -> Option<Panic> {
    let exc_val = mrb.pending_exc();
    if exc_val.is_nil() {
        return None;
    }
    let class_name = {
        let cn = exc_val.classname(mrb);
        if cn.is_empty() {
            "RuntimeError".to_string()
        } else {
            cn.to_string()
        }
    };
    let message = {
        let msg_val = exc_val.call(mrb, c"message", &[]);
        let m = msg_val.to_string(mrb);
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
    }

    #[test]
    fn origin_for_class_defaults_to_sandbox() {
        assert_eq!(origin_for_class("RuntimeError"), "sandbox");
        assert_eq!(origin_for_class("Kobako::Transport::WireError"), "sandbox");
        assert_eq!(origin_for_class("NoMethodError"), "sandbox");
    }
}
