//! The core envelope — the fixed-layout outer frame of every host↔guest
//! message, and the guest's independent implementation of it.
//!
//! Routing and outcome attribution live here; everything a resolved method
//! consumes rides through as an opaque `payload` this layer never reads.
//! A guest whose host shares another schema keeps this module and replaces
//! only the payload codec.
//!
//! `crates/kobako-runtime` carries the host-side implementation of the
//! same layout, written separately so the two cross-check each other.
//!
//! [core envelope]: ../../../docs/wire/envelope.md

pub mod bytes;
pub mod call;
pub mod error_record;
pub mod invocation;
pub mod outcome;
pub mod reply;

pub use call::{Call, Target};
pub use error_record::ErrorRecord;
pub use invocation::{Preamble, Run, Snippet, Snippets};
pub use outcome::{Outcome, Panic, ORIGIN_SANDBOX, ORIGIN_SERVICE};
pub use reply::{Reply, YieldReply};

use core::fmt;

/// A message that does not conform to the core envelope layout. The reason
/// is a fixed string because every case is a wire violation the receiving
/// side rejects rather than a condition a caller recovers from by
/// inspecting it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Error(pub &'static str);

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

impl std::error::Error for Error {}
