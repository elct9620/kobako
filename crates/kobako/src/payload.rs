//! What a `#run` carries into the guest.
//!
//! A payload is either bytes that are already final, or a builder the
//! verb runs once the invocation's Handle table exists. The second form
//! is not a convenience: a payload naming a host object carries the id
//! that invocation's table issued for it, and that table does not exist
//! until the invocation has begun.
//!
//! The verb takes this type rather than a codec's shape, so which schema
//! spells a payload is a choice made here — at a constructor — instead of
//! at the verb. `values` is the bundled codec's spelling; a host with its
//! own schema builds its bytes and hands them over.

use std::sync::Mutex;

use crate::error::Error;
use crate::handles::{HandleTable, Handles};

/// The payload one `run` carries.
///
/// Opaque: how a payload reaches its bytes is this crate's business, so
/// a later way of producing them adds a constructor rather than changing
/// what callers match on.
pub struct RunPayload<'a>(Source<'a>);

/// The caller's step from this invocation's table to finished bytes.
type Finish<'a> = Box<dyn FnOnce(&Handles<'_>) -> Result<Vec<u8>, Error> + 'a>;

enum Source<'a> {
    /// Bytes the caller already holds — nothing in them names a host
    /// object, so no table is needed to finish them.
    Ready(Vec<u8>),
    /// Bytes the caller finishes once the invocation's table exists.
    Deferred(Finish<'a>),
}

impl<'a> RunPayload<'a> {
    /// A payload that is already complete.
    pub fn bytes(bytes: impl Into<Vec<u8>>) -> Self {
        RunPayload(Source::Ready(bytes.into()))
    }

    /// A payload built against this invocation's Handle table — the form
    /// to use when the entrypoint is handed a capability, since the id
    /// standing for it is issued by that table.
    pub fn build(build: impl FnOnce(&Handles<'_>) -> Result<Vec<u8>, Error> + 'a) -> Self {
        RunPayload(Source::Deferred(Box::new(build)))
    }

    /// Finish the payload against the invocation's table.
    pub(crate) fn encode(self, table: &Mutex<HandleTable>) -> Result<Vec<u8>, Error> {
        match self.0 {
            Source::Ready(bytes) => Ok(bytes),
            Source::Deferred(build) => build(&Handles::new(table)),
        }
    }
}
