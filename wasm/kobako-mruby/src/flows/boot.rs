//! Boot helpers shared by `__kobako_eval` and `__kobako_run`.
//!
//! Both entry points acquire a VM in the canonical boot state —
//! reusing the slot a build-time pre-initialized image baked, or
//! booting lazily — then materialise
//! the Frame 1 preamble's proxy classes and replay any preloaded Frame 3
//! snippets before running the entry-specific body. When any of those
//! steps fails, the failure surfaces as a Panic with
//! `origin = sandbox` and `name = "Kobako::BootError"` — this module
//! centralises both the orchestration and every Panic shape the entry
//! flows build a host-visible failure from.
//!
//! Snippet replay compiles each snippet under a
//! `(snippet:Name)` filename so any uncaught exception's backtrace
//! attributes back to the originating `#preload` call. Replay failures
//! are always sandbox-origin even when the raised class would otherwise
//! map to "service" — preloaded snippets are sandbox code.

#[cfg(mruby_linked)]
use crate::runtime::Kobako;
#[cfg(mruby_linked)]
use beni::Ccontext;
#[cfg(mruby_linked)]
use beni::Mrb;
#[cfg(mruby_linked)]
use kobako_transport::envelope::{Bindings, Snippet, Snippets};
use kobako_transport::envelope::{ErrorRecord, Origin, Panic};

/// Build a Panic envelope carrying the kobako boot defaults
/// (`origin = sandbox`, `name = "Kobako::BootError"`, empty
/// backtrace, no correction to offer). The exclusive constructor for the
/// `Kobako::BootError` panic shape — every boot-time failure should
/// pass through here so the host-visible attribution stays uniform.
pub(super) fn boot_panic(message: impl Into<String>) -> Panic {
    sandbox_panic("Kobako::BootError", message)
}

/// Build a Panic envelope for a wire-layer failure at the invocation
/// boundary (`origin = sandbox`, `name = "Kobako::Transport::Error"`,
/// empty backtrace, no correction to offer). The exclusive constructor for the
/// `Kobako::Transport::Error` panic shape — the sibling of `boot_panic`
/// for decode / encode faults on the invocation channel's envelopes, so
/// the host-visible attribution stays uniform.
pub(super) fn transport_panic(message: impl Into<String>) -> Panic {
    sandbox_panic("Kobako::Transport::Error", message)
}

/// Frame a codec refusal as the Panic the host reads it as. The class and
/// wording are the refusal's; this only gives them the invocation
/// boundary's envelope shape.
#[cfg(mruby_linked)]
pub(super) fn panic_for(refusal: &crate::refusal::Refusal) -> Panic {
    sandbox_panic(refusal.class, refusal.message.clone())
}

/// The shape every host-detected failure at the invocation boundary
/// shares: sandbox origin, no backtrace (the failure is the host's
/// reading of the wire, not a guest stack), no correction to offer.
#[cfg(any(mruby_linked, test))]
fn sandbox_panic(class: &str, message: impl Into<String>) -> Panic {
    Panic {
        origin: Origin::Sandbox,
        error: ErrorRecord {
            name: class.into(),
            message: message.into(),
            backtrace: Vec::new(),
        },
        available: Vec::new(),
    }
}

/// Serialize `result_val` as the invocation's ok Outcome — or the
/// matching Panic when the value has no wire representation or the
/// envelope encode fails. The shared tail of the eval and run entry
/// bodies, so the outcome attribution cannot drift between them.
#[cfg(mruby_linked)]
pub(super) fn write_value_outcome<G: crate::MrbGuest>(kobako: &Kobako, result_val: beni::Value) {
    use crate::codec::PayloadCodec;
    use crate::refusal::Position;
    use kobako_core::abi::{write_outcome, write_panic};
    use kobako_transport::envelope::Outcome;

    match G::Codec::encode_value(kobako, result_val) {
        Ok(payload) => write_outcome(Outcome::Ok(payload).encode()),
        Err(err) => write_panic(panic_for(&crate::refusal::at(
            Position::InvocationValue,
            err,
        ))),
    }
}

/// Attribute a Panic from the mruby exception class that produced it.
/// Mirrors the host-side rules — a `Kobako::ServiceError` raised from a
/// Service capability attributes to the Service; everything else to the
/// sandbox. Pure string inspection — host-buildable for unit tests.
pub(super) fn origin_for_class(class_name: &str) -> Origin {
    if class_name.contains("ServiceError") {
        Origin::Service
    } else {
        Origin::Sandbox
    }
}

/// Read Frame 1 from stdin and decode it into the bind-path list.
/// Either step failing surfaces as a `boot_panic`.
#[cfg(mruby_linked)]
pub(super) fn read_preamble() -> Result<Vec<String>, Panic> {
    let bytes = kobako_core::frames::read_frame()
        .ok_or_else(|| boot_panic("failed to read the Sandbox setup data"))?;
    Bindings::decode(&bytes)
        .map(|bindings| bindings.paths)
        .map_err(|_| boot_panic("failed to decode the Sandbox setup data"))
}

/// Read Frame 3 from stdin and decode it into the snippet list.
#[cfg(mruby_linked)]
pub(super) fn read_snippets() -> Result<Vec<Snippet>, Panic> {
    let bytes = kobako_core::frames::read_frame()
        .ok_or_else(|| boot_panic("failed to read the preloaded snippets"))?;
    Snippets::decode(&bytes)
        .map(|snippets| snippets.entries)
        .map_err(|_| boot_panic("failed to decode the preloaded snippets"))
}

/// Open an mruby VM into the empty `super::mrb_slot::MRB` slot and
/// install the Kobako runtime plus the shell gem set — producing the
/// canonical boot state. On `Err` the slot is
/// cleared so no caller observes a half-set state.
#[cfg(mruby_linked)]
pub(super) fn boot_vm<G: crate::MrbGuest>() -> Result<(), Panic> {
    let mrb = Mrb::open().map_err(|_| boot_panic("failed to start the Sandbox interpreter"))?;
    super::mrb_slot::MRB.install(mrb);
    let mrb = super::mrb_slot::MRB
        .as_ref()
        .expect("MRB just installed above");
    if let Err(e) = Kobako::init::<G>(mrb) {
        let panic = boot_panic(format!(
            "Sandbox boot registration failed: {}",
            e.message(mrb)
        ));
        super::mrb_slot::MRB.clear();
        return Err(panic);
    }
    Ok(())
}

/// Hand the entry flow a VM in the canonical boot state: reuse
/// the slot the Guest Binary's pre-initialized image baked, or boot
/// lazily when the artifact carries none. Returns the `Kobako` token
/// for the live VM.
#[cfg(mruby_linked)]
pub(super) fn acquire_vm<G: crate::MrbGuest>() -> Result<Kobako, Panic> {
    if super::mrb_slot::MRB.as_ref().is_none() {
        boot_vm::<G>()?;
    }
    let mrb = super::mrb_slot::MRB
        .as_ref()
        .expect("slot populated by the baked image or boot_vm above");
    // SAFETY: the slot only ever holds a VM that passed `Kobako::init`
    // — baked at build time (`bake_boot`) or booted by `boot_vm` above.
    Ok(unsafe { Kobako::resolve_raw(mrb) })
}

/// Bake the canonical boot state into the running instance —
/// the body behind `MrbGuest::bake_boot`, called by the build-time
/// wizer pre-initialization entry. Panics on failure so a bake aborts
/// loudly instead of shipping a half-booted image.
#[cfg(mruby_linked)]
pub(crate) fn bake_boot<G: crate::MrbGuest>() {
    if let Err(panic) = boot_vm::<G>() {
        panic!("canonical boot state bake failed: {}", panic.error.message);
    }
}

/// Materialise the bound-constant proxy classes from the Frame 1
/// `paths` onto the invocation's VM.
#[cfg(mruby_linked)]
pub(super) fn install_preamble(kobako: &Kobako, paths: &[String]) -> Result<(), Panic> {
    kobako
        .install_bindings(paths)
        .map_err(|err| boot_panic(err.to_string()))
}

/// Replay every snippet in `snippets` against `kobako`'s VM in
/// insertion order so any uncaught exception's backtrace attributes
/// back to the originating `#preload` call. Source entries
/// load via a fresh ccontext under `(snippet:Name)` filenames; bytecode
/// entries load through beni's `Mrb::load_bytecode` (the filename,
/// when present, is baked into their RITE `debug_info` section). The first
/// snippet that raises wins: the resulting Panic carries that snippet's
/// class / message / backtrace and is forced to sandbox origin even
/// when `origin_for_class` would have chosen `"service"`. Bytecode
/// entries whose load returned a structural-failure code
/// additionally override the panic class to `Kobako::BytecodeError`;
/// a successful load that then raised at top level keeps the
/// natural mruby class.
#[cfg(mruby_linked)]
pub(super) fn replay_snippets(kobako: &Kobako, snippets: &[Snippet]) -> Result<(), Panic> {
    let mrb = kobako.mrb();
    for entry in snippets {
        let load = match entry {
            Snippet::Source { name, body } => {
                load_source_snippet(mrb, name, body)?;
                BytecodeLoad::Loaded
            }
            Snippet::Bytecode { body } => load_bytecode_snippet(mrb, body),
        };
        if let Some(panic) = take_pending_panic(kobako) {
            return Err(reshape_replay_panic(panic, load));
        }
    }
    Ok(())
}

/// Apply the replay-specific reshape to a pending Panic. Replay-time
/// failures are always sandbox origin even when the class would
/// normally map to service. Structural failures further
/// override the class to `Kobako::BytecodeError`; a bytecode snippet
/// that loaded cleanly and then raised at top level keeps the
/// natural mruby class preserved. Functional struct-update keeps the
/// reshape in one expression — no mid-life mutation of the panic
/// fields.
#[cfg(mruby_linked)]
fn reshape_replay_panic(panic: Panic, load: BytecodeLoad) -> Panic {
    let name = match load {
        BytecodeLoad::StructuralFailure => "Kobako::BytecodeError".into(),
        BytecodeLoad::Loaded => panic.error.name,
    };
    Panic {
        origin: Origin::Sandbox,
        error: ErrorRecord {
            name,
            ..panic.error
        },
        ..panic
    }
}

/// Outcome of a bytecode-form snippet load. Distinguishes the two
/// failure shapes the caller's class-override step needs to tell
/// apart: a successful parse (whose top-level execution may still have
/// raised, natural mruby class preserved) from a structural
/// failure on the RITE header / IREP body, which gets promoted to
/// `Kobako::BytecodeError`.
#[cfg(mruby_linked)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BytecodeLoad {
    Loaded,
    StructuralFailure,
}

/// Compile and execute a source snippet under a fresh ccontext whose
/// filename is `(snippet:Name)`. Surfaces ccontext allocation failure
/// as a `boot_panic`; any mruby compile / runtime fault is left in
/// `mrb->exc` for the shared `take_pending_panic` step. A snippet
/// `name` carrying an interior NUL byte (wire violation) also fails
/// through `boot_panic` since `CString::new` rejects it.
#[cfg(mruby_linked)]
fn load_source_snippet(mrb: &Mrb, name: &str, body: &str) -> Result<(), Panic> {
    let filename = std::ffi::CString::new(format!("(snippet:{})", name))
        .map_err(|_| boot_panic("snippet name contains an invalid character"))?;
    let Some(cxt) = Ccontext::new(mrb, &filename) else {
        return Err(boot_panic("failed to initialize the Sandbox interpreter"));
    };
    cxt.load_nstring(body.as_bytes());
    // `cxt` drops here — `mrb_ccontext_free` runs automatically.
    Ok(())
}

/// Execute a precompiled RITE bytecode blob via beni's
/// `Mrb::load_bytecode`. Returns `BytecodeLoad::Loaded` when the
/// IREP parsed (even if its top-level execution then raised)
/// and `BytecodeLoad::StructuralFailure` when the RITE header / IREP
/// body failed structural validation. Either way, a
/// pending exception is left in `mrb->exc` for the shared
/// `take_pending_panic` step. Folding the return code into a typed
/// enum keeps the `c_int` from leaking into the replay control flow.
#[cfg(mruby_linked)]
fn load_bytecode_snippet(mrb: &Mrb, body: &[u8]) -> BytecodeLoad {
    if mrb.load_bytecode(body) == 0 {
        BytecodeLoad::Loaded
    } else {
        BytecodeLoad::StructuralFailure
    }
}

/// If an mruby exception is pending on `kobako`'s VM, extract its
/// class name, message, and backtrace into a Panic envelope (with
/// `origin` chosen by `origin_for_class`). Returns `None` when no
/// exception is pending. Clears `mrb->exc` via `Mrb::clear_exc`
/// before returning.
#[cfg(mruby_linked)]
pub(super) fn take_pending_panic(kobako: &Kobako) -> Option<Panic> {
    let mrb = kobako.mrb();
    let exc_val = mrb.pending_exc();
    if exc_val.is_nil() {
        return None;
    }
    let panic = panic_from_exception(kobako, exc_val);
    mrb.clear_exc();
    Some(panic)
}

/// Extract `(class, message, backtrace)` from an mruby exception value
/// — the fields every host-visible error shape carries, whichever
/// envelope wraps them (the Panic here, the yield error map in
/// `yield_block`). Each step reads `exc_val` while it is still
/// GC-reachable in mruby's arena; an empty classname degrades to
/// `RuntimeError`, and the `message` accessor itself raising (or
/// returning empty) degrades to the class name rather than recursing
/// into another failure.
#[cfg(mruby_linked)]
pub(super) fn exception_fields(
    kobako: &Kobako,
    exc_val: beni::Value,
) -> (String, String, Vec<String>) {
    let mrb = kobako.mrb();
    let class_name = {
        let cn = exc_val.classname(mrb);
        if cn.is_empty() {
            "RuntimeError".to_string()
        } else {
            cn.to_string()
        }
    };
    let message = {
        let msg_val = exc_val
            .funcall(mrb, c"message", &[])
            .unwrap_or(beni::Value::nil());
        let m = msg_val.to_string(mrb);
        if m.is_empty() {
            class_name.clone()
        } else {
            m
        }
    };
    let backtrace = kobako.extract_backtrace(exc_val);
    (class_name, message, backtrace)
}

/// Build a Panic envelope from an mruby exception value, with `origin`
/// chosen by `origin_for_class`. Shared by `take_pending_panic` (the
/// `mrb->exc`-set path a source / bytecode load leaves) and
/// `panic_from_error` (the `Err` a protected funcall returns).
#[cfg(mruby_linked)]
fn panic_from_exception(kobako: &Kobako, exc_val: beni::Value) -> Panic {
    let (class, message, backtrace) = exception_fields(kobako, exc_val);
    Panic {
        origin: origin_for_class(&class),
        error: ErrorRecord {
            name: class,
            message,
            backtrace,
        },
        available: Vec::new(),
    }
}

/// Fold a `beni::Error` a protected funcall returns into a Panic
/// envelope. A raised Ruby exception reuses `panic_from_exception`; a
/// Rust-side `Error::Panic` becomes a sandbox-origin `RuntimeError`.
#[cfg(mruby_linked)]
pub(super) fn panic_from_error(kobako: &Kobako, err: beni::Error) -> Panic {
    match err {
        beni::Error::Exception(exc) => panic_from_exception(kobako, exc),
        beni::Error::Panic(message) => sandbox_panic("RuntimeError", message),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boot_panic_carries_kobako_boot_defaults() {
        let p = boot_panic("failed to read preamble frame");
        assert_eq!(p.origin, Origin::Sandbox);
        assert_eq!(p.error.name, "Kobako::BootError");
        assert_eq!(p.error.message, "failed to read preamble frame");
        assert!(p.error.backtrace.is_empty());
        assert!(p.available.is_empty());
    }

    #[test]
    fn transport_panic_carries_kobako_transport_defaults() {
        let p = transport_panic("failed to decode the invocation request");
        assert_eq!(p.origin, Origin::Sandbox);
        assert_eq!(p.error.name, "Kobako::Transport::Error");
        assert_eq!(p.error.message, "failed to decode the invocation request");
        assert!(p.error.backtrace.is_empty());
        assert!(p.available.is_empty());
    }

    #[test]
    fn origin_for_class_routes_service_errors_to_service() {
        assert_eq!(origin_for_class("Kobako::ServiceError"), Origin::Service);
    }

    #[test]
    fn origin_for_class_defaults_to_sandbox() {
        assert_eq!(origin_for_class("RuntimeError"), Origin::Sandbox);
        assert_eq!(
            origin_for_class("Kobako::Transport::Error"),
            Origin::Sandbox
        );
        assert_eq!(origin_for_class("NoMethodError"), Origin::Sandbox);
    }
}
