//! Isolation profile — what a frontend requests and a runtime builds and
//! declares.
//!
//! One of an ordered set: the host application requests the profile it
//! wants, the runtime builds it and declares the profile it actually
//! built, and the frontend refuses a declaration below the request — so
//! the request is also the floor.

use std::time::Instant;

/// The ordered isolation profiles a runtime builds one of.
///
/// `Hermetic` is the full ambient-denial profile: ambient time and
/// entropy denied at the WASI boundary, and no ambient filesystem /
/// environment / network reachability: only a Grant reaches a file or a
/// socket. `Permissive` differs only in the ambient
/// authority it grants — live time and entropy at the WASI boundary — and in
/// the times and inode numbers it shows of an entry. Ordering
/// follows strength (`Permissive < Hermetic`), so a floor check is a
/// plain comparison.
///
/// Deliberately not `#[non_exhaustive]`, unlike this crate's error
/// types. A wildcard arm here would let a runtime silently keep building
/// the profile it already knows while declaring it satisfied one it never
/// implemented; the compile error a new profile causes is what forces
/// each runtime to answer for it. The ordering is the same reason — a
/// profile inserted between two changes what every existing `>=` means, which
/// no wildcard could have absorbed either.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Profile {
    Permissive,
    Hermetic,
}

impl Profile {
    /// When a guest's clock, due `at` in host time, is due in a wait on this
    /// profile, whichever ABI a Wasm Driver encodes the wait into. A hermetic
    /// guest's clocks never advance, so a wait on them alone is due at once;
    /// beside a host-side producer, which only host time brings, as
    /// `has_producer` says, it is due at `at` as a live clock is.
    pub fn clock_due(self, at: Instant, has_producer: bool) -> Instant {
        match (self, has_producer) {
            (Self::Hermetic, false) => Instant::now(),
            _ => at,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // @behavior RT-014
    #[test]
    fn a_declaration_satisfies_any_floor_at_or_below_it() {
        assert!(Profile::Hermetic >= Profile::Hermetic);
        assert!(Profile::Hermetic >= Profile::Permissive);
        assert!(Profile::Permissive < Profile::Hermetic);
    }
}
