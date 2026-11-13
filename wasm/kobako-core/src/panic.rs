//! The Panic a guest answers for a failure it detected itself.

use kobako_transport::envelope::{ErrorRecord, Origin, Panic};

/// A failure the guest detected itself, under the class `class` the host
/// raises for it. No backtrace: no script stack stands behind it.
pub fn sandbox_panic(class: &str, message: impl Into<String>) -> Panic {
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
