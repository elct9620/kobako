//! The core envelope — the fixed-layout outer frame of every host↔guest
//! message.
//!
//! Routing and outcome attribution live here; everything a resolved method
//! consumes rides through as an opaque `payload` this layer never reads. A
//! tier that only routes messages therefore needs no payload codec at all,
//! and the one an assembly does use is its own choice.
//!
//! A decoded envelope borrows the buffer it came from, so a payload reaches
//! its reader as a view rather than a copy. The two fields a reader keeps
//! past that buffer's life — a binding's path and a snippet's body, read
//! from a frame at boot and consumed later — are copied at decode.
//!
//! This layer has one implementation, so its `golden_layout_*` tests are
//! what hold it to the layout document. They spell each tag as the literal
//! byte that document fixes, never as the constant beside it: a golden
//! written from the constant compares the implementation to itself.
//!
//! [core envelope]: ../../../docs/wire/envelope.md

pub(crate) mod bytes;
pub mod call;
pub mod error_record;
pub mod fault;
pub mod invocation_frames;
pub mod outcome;
pub mod reply;
pub mod run;

pub use call::{Call, Target};
pub use error_record::ErrorRecord;
pub use fault::{Fault, FaultKind};
pub use invocation_frames::{Bindings, Snippet, Snippets};
pub use outcome::{Origin, Outcome, Panic};
pub use reply::{Reply, YieldReply};
pub use run::Run;

use std::fmt;

/// A message that does not conform to the core envelope layout. Only
/// decoding produces one — every `encode` is infallible — so the name says
/// which direction it comes from.
///
/// The reason is a fixed string because every case is a wire violation the
/// receiving side rejects rather than a condition a caller recovers from by
/// inspecting it; it stays behind `message` so a later reason can carry more
/// than a literal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecodeError(&'static str);

impl DecodeError {
    pub const fn new(reason: &'static str) -> Self {
        DecodeError(reason)
    }

    pub fn message(&self) -> &str {
        self.0
    }
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

impl std::error::Error for DecodeError {}
