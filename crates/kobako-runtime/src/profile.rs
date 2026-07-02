//! Isolation profile — the posture a runtime declares.
//!
//! A declaration on an ordered ladder, not a switch: a runtime names
//! the strongest posture it provides, a frontend compares that
//! declaration against the floor its host application requested, and
//! nothing about the runtime's behavior changes with the floor. The
//! governing contract lives in the spec corpus
//! (docs/behavior/security.md).

/// The ordered isolation ladder a runtime declares one rung of.
///
/// `Hermetic` is the full ambient-denial posture: ambient time and
/// entropy denied at the WASI boundary, no filesystem / environment /
/// network reachability, and no host import beyond the wire ABI's
/// `__kobako_dispatch`. `Permissive` declares nothing beyond the Wasm
/// memory cell. Ordering follows strength (`Permissive < Hermetic`),
/// so a floor check is a plain comparison.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Profile {
    Permissive,
    Hermetic,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ladder_orders_permissive_below_hermetic() {
        assert!(Profile::Permissive < Profile::Hermetic);
    }

    #[test]
    fn a_declaration_satisfies_any_floor_at_or_below_it() {
        assert!(Profile::Hermetic >= Profile::Hermetic);
        assert!(Profile::Hermetic >= Profile::Permissive);
        assert!(!(Profile::Permissive >= Profile::Hermetic));
    }
}
