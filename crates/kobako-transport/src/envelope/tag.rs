//! The tag registry: the first byte of every message on either channel.
//!
//! One number space covers requests and answers alike, so a byte means
//! one thing in every message and an answer's arm never collides across
//! operations. The registry grows append-only; a receiver answers a
//! request tag it does not serve as unsupported.

/// Never assigned, so a zeroed message cannot pass for one that was written.
pub(crate) const INVALID: u8 = 0x00;
pub(crate) const EVAL: u8 = 0x01;
pub(crate) const RUN: u8 = 0x02;
pub(crate) const YIELD: u8 = 0x03;
pub(crate) const CALL: u8 = 0x04;

pub(crate) const OK: u8 = 0x10;
pub(crate) const FAULT: u8 = 0x11;
pub(crate) const BREAK: u8 = 0x12;
pub(crate) const ERROR: u8 = 0x13;
pub(crate) const PANIC: u8 = 0x14;
pub(crate) const UNSUPPORTED: u8 = 0x15;

/// Whether the registry gives `tag` a meaning, the invalid tag included.
/// An assigned tag means one thing in every message, so it can never
/// stand for an operation this registry does not know.
pub(crate) fn is_assigned(tag: u8) -> bool {
    matches!(
        tag,
        INVALID | EVAL | RUN | YIELD | CALL | OK | FAULT | BREAK | ERROR | PANIC | UNSUPPORTED
    )
}
