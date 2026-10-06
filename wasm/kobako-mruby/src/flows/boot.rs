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

use crate::runtime::Kobako;
use beni::Ccontext;
use beni::Mrb;
use kobako_transport::envelope::{Bindings, ErrorRecord, Origin, Panic, Snippet, Snippets};

/// Every boot-time failure passes through here, so the host-visible
/// attribution stays uniform.
pub(super) fn boot_panic(message: impl Into<String>) -> Panic {
    sandbox_panic("Kobako::BootError", message)
}

/// Every envelope decode or encode fault on the invocation channel passes
/// through here, so the host-visible attribution stays uniform.
pub(super) fn transport_panic(message: impl Into<String>) -> Panic {
    sandbox_panic("Kobako::Transport::Error", message)
}

/// The class and wording are the refusal's; this only gives them the
/// invocation boundary's envelope shape.
pub(super) fn panic_for(refusal: &crate::refusal::Refusal) -> Panic {
    sandbox_panic(refusal.class, refusal.message.clone())
}

/// No backtrace: the failure is a reading of the wire, not a guest stack.
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

/// Shared by the eval and run entries, so the outcome attribution cannot
/// drift between them.
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

/// The classes a dispatch raises when a Service call fails — the names
/// `Kobako::class_for` picks from. Named rather than matched by shape:
/// a guest is free to define its own `ServiceError`, and what it calls
/// its exceptions must not decide what the host attributes them to.
const SERVICE_ERROR_CLASSES: [&str; 3] = [
    "Kobako::ServiceError",
    "Kobako::NoServiceError",
    "Kobako::ServiceArgumentError",
];

/// Mirrors the host-side rules: an exception a Service capability raised
/// attributes to the Service, everything else to the sandbox.
pub(super) fn origin_for_class(class_name: &str) -> Origin {
    if SERVICE_ERROR_CLASSES.contains(&class_name) {
        Origin::Service
    } else {
        Origin::Sandbox
    }
}

pub(super) fn read_preamble() -> Result<Vec<String>, Panic> {
    let bytes = kobako_core::frames::read_frame()
        .ok_or_else(|| boot_panic("failed to read the Sandbox setup data"))?;
    Bindings::decode(&bytes)
        .map(|bindings| bindings.paths)
        .map_err(|_| boot_panic("failed to decode the Sandbox setup data"))
}

pub(super) fn read_snippets() -> Result<Vec<Snippet>, Panic> {
    let bytes = kobako_core::frames::read_frame()
        .ok_or_else(|| boot_panic("failed to read the preloaded snippets"))?;
    Snippets::decode(&bytes)
        .map(|snippets| snippets.entries)
        .map_err(|_| boot_panic("failed to decode the preloaded snippets"))
}

/// On `Err` the slot is cleared, so no caller observes a half-set state.
pub(super) fn boot_vm<G: crate::MrbGuest>() -> Result<(), Panic> {
    let mrb = Mrb::open().map_err(|_| boot_panic("failed to start the Sandbox interpreter"))?;
    super::mrb_slot::MRB.install(mrb);
    let mrb = super::mrb_slot::MRB
        .as_ref()
        .expect("MRB just installed above");
    let kobako = match Kobako::init::<G>(mrb) {
        Ok(kobako) => kobako,
        Err(e) => {
            let panic = boot_panic(format!(
                "Sandbox boot registration failed: {}",
                e.message(mrb)
            ));
            super::mrb_slot::MRB.clear();
            return Err(panic);
        }
    };
    // Recorded on the success path only: an unresolved entrypoint's
    // correction is measured against this, and the bake reaches here.
    super::boot_constants::record(&kobako);
    Ok(())
}

/// Reuses the VM the pre-initialized image baked, or boots lazily when the
/// artifact carries none.
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

/// Panics on failure, so a bake aborts loudly instead of shipping a
/// half-booted image.
pub(crate) fn bake_boot<G: crate::MrbGuest>() {
    if let Err(panic) = boot_vm::<G>() {
        panic!("canonical boot state bake failed: {}", panic.error.message);
    }
}

pub(super) fn install_preamble(kobako: &Kobako, paths: &[String]) -> Result<(), Panic> {
    kobako
        .install_bindings(paths)
        .map_err(|err| boot_panic(err.to_string()))
}

/// A failing snippet's Panic is forced to sandbox origin even when its
/// class would choose the Service: preloaded snippets are sandbox code.
pub(super) fn replay_snippets(kobako: &Kobako, snippets: &[Snippet]) -> Result<(), Panic> {
    for entry in snippets {
        match entry {
            Snippet::Source { name, body } => load_source_snippet(kobako, name, body)?,
            Snippet::Bytecode { body } => load_bytecode_snippet(kobako, body)?,
        }
    }
    Ok(())
}

fn replay_panic(panic: Panic) -> Panic {
    Panic {
        origin: Origin::Sandbox,
        ..panic
    }
}

/// The `(snippet:Name)` filename lets a backtrace point back at the
/// originating `#preload` call.
fn load_source_snippet(kobako: &Kobako, name: &str, body: &str) -> Result<(), Panic> {
    let filename = std::ffi::CString::new(format!("(snippet:{})", name))
        .map_err(|_| boot_panic("snippet name contains an invalid character"))?;
    let Some(cxt) = Ccontext::new(kobako.mrb(), &filename) else {
        return Err(boot_panic("failed to initialize the Sandbox interpreter"));
    };
    cxt.load_nstring(body.as_bytes())
        .map(drop)
        .map_err(|err| replay_panic(load_panic(kobako, &filename, err)))
}

/// The class a bytecode load answers when the blob fails its
/// structural check — `ScriptError` itself, so a subclass the
/// program raised keeps its own name.
const STRUCTURAL_FAILURE: &str = "ScriptError";

/// A blob that fails its structural check is promoted to
/// `Kobako::BytecodeError`; a program that loaded and then raised keeps the
/// class it raised.
fn load_bytecode_snippet(kobako: &Kobako, body: &[u8]) -> Result<(), Panic> {
    let Err(err) = kobako.mrb().load_bytecode(body) else {
        return Ok(());
    };
    let panic = replay_panic(panic_from_error(kobako, err));
    if panic.error.name != STRUCTURAL_FAILURE {
        return Err(panic);
    }
    Err(Panic {
        error: ErrorRecord {
            name: "Kobako::BytecodeError".into(),
            ..panic.error
        },
        ..panic
    })
}

/// A parse failure names where the parse stopped, since a program that
/// never ran has no backtrace to locate it.
pub(super) fn load_panic(kobako: &Kobako, filename: &core::ffi::CStr, err: beni::Error) -> Panic {
    let beni::Error::Syntax(parse) = err else {
        return panic_from_error(kobako, err);
    };
    let message = if parse.message().is_empty() {
        "syntax error".to_string()
    } else {
        format!(
            "{}:{}:{}: {}",
            filename.to_string_lossy(),
            parse.line(),
            parse.column(),
            parse.message()
        )
    };
    sandbox_panic("SyntaxError", message)
}

/// Each step reads `exc_val` while it is still GC-reachable in mruby's
/// arena. A `message` accessor that raises degrades to the class name
/// rather than recursing into another failure.
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

pub(super) fn panic_from_error(kobako: &Kobako, err: beni::Error) -> Panic {
    match err {
        beni::Error::Exception(exc) => panic_from_exception(kobako, exc),
        beni::Error::Syntax(_) => {
            unreachable!("only a load answers a parse failure, and `load_panic` folds it")
        }
        beni::Error::Panic(message) => sandbox_panic("RuntimeError", message),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // @behavior OC-043
    #[test]
    fn boot_panic_carries_kobako_boot_defaults() {
        let p = boot_panic("failed to read preamble frame");
        assert_eq!(p.origin, Origin::Sandbox);
        assert_eq!(p.error.name, "Kobako::BootError");
        assert_eq!(p.error.message, "failed to read preamble frame");
        assert!(p.error.backtrace.is_empty());
        assert!(p.available.is_empty());
    }

    // @behavior OC-044
    #[test]
    fn transport_panic_carries_kobako_transport_defaults() {
        let p = transport_panic("failed to decode the invocation request");
        assert_eq!(p.origin, Origin::Sandbox);
        assert_eq!(p.error.name, "Kobako::Transport::Error");
        assert_eq!(p.error.message, "failed to decode the invocation request");
        assert!(p.error.backtrace.is_empty());
        assert!(p.available.is_empty());
    }

    // @behavior OC-045
    #[test]
    fn origin_for_class_routes_every_service_error_to_service() {
        for class in SERVICE_ERROR_CLASSES {
            assert_eq!(
                origin_for_class(class),
                Origin::Service,
                "{class} is raised by a failed Service call, so a Panic naming it must \
                 attribute to the Service"
            );
        }
    }

    // @behavior OC-046
    #[test]
    fn origin_for_class_defaults_to_sandbox() {
        assert_eq!(origin_for_class("RuntimeError"), Origin::Sandbox);
        assert_eq!(
            origin_for_class("Kobako::Transport::Error"),
            Origin::Sandbox
        );
        assert_eq!(origin_for_class("NoMethodError"), Origin::Sandbox);
    }

    // @behavior OC-047
    #[test]
    fn a_guests_own_error_named_like_kobakos_does_not_claim_service_origin() {
        assert_eq!(
            origin_for_class("MyApp::ServiceError"),
            Origin::Sandbox,
            "a class the guest defined must not reach Service attribution by resembling \
             the name of one kobako raises"
        );
    }
}
