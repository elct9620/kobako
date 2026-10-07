//! How a failed invocation reaches the host: the Panic every flow builds,
//! and the rules that attribute an mruby exception to the sandbox or to a
//! Service.

use crate::refusal::{Refusal, SANDBOX_ERROR, TRANSPORT_ERROR};
use crate::runtime::Kobako;
use kobako_transport::envelope::{ErrorRecord, Origin, Panic};

/// Every boot-time failure passes through here, so the host-visible
/// attribution stays uniform.
pub(super) fn boot_panic(message: impl Into<String>) -> Panic {
    sandbox_panic("Kobako::BootError", message)
}

/// Every envelope decode or encode fault on the invocation channel passes
/// through here, so the host-visible attribution stays uniform.
pub(super) fn transport_panic(message: impl Into<String>) -> Panic {
    sandbox_panic(TRANSPORT_ERROR, message)
}

/// The class and wording are the refusal's; this only gives them the
/// invocation boundary's envelope shape.
pub(super) fn panic_for(refusal: &Refusal) -> Panic {
    sandbox_panic(refusal.class, refusal.message.clone())
}

/// A failure the flow itself detected, outside any guest exception.
pub(super) fn flow_panic(message: impl Into<String>) -> Panic {
    sandbox_panic(SANDBOX_ERROR, message)
}

/// No backtrace: the flow detected this failure itself, so no guest stack
/// stands behind it.
pub(super) fn sandbox_panic(class: &str, message: impl Into<String>) -> Panic {
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
