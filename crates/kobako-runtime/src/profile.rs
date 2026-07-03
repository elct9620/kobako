//! Isolation profile — the posture a frontend requests and a runtime
//! builds and declares.
//!
//! A rung on an ordered ladder: the host application requests the
//! posture it wants, the runtime builds it and declares the posture it
//! actually built, and the frontend refuses a declaration below the
//! request — so the request is also the floor. The governing contract
//! lives in the spec corpus (docs/behavior/security.md).

/// The ordered isolation ladder a runtime builds one rung of.
///
/// `Hermetic` is the full ambient-denial posture: ambient time and
/// entropy denied at the WASI boundary, no filesystem / environment /
/// network reachability, and no host import beyond the wire ABI's
/// `__kobako_dispatch`. `Permissive` differs in exactly one grant —
/// live ambient time and entropy at the WASI boundary. Ordering
/// follows strength (`Permissive < Hermetic`), so a floor check is a
/// plain comparison.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Profile {
    Permissive,
    Hermetic,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_declaration_satisfies_any_floor_at_or_below_it() {
        assert!(Profile::Hermetic >= Profile::Hermetic);
        assert!(Profile::Hermetic >= Profile::Permissive);
        assert!(Profile::Permissive < Profile::Hermetic);
    }
}
