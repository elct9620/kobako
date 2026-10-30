//! Isolation profile — the posture a frontend requests and a runtime
//! builds and declares.
//!
//! A rung on an ordered ladder: the host application requests the
//! posture it wants, the runtime builds it and declares the posture it
//! actually built, and the frontend refuses a declaration below the
//! request — so the request is also the floor.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::filesystem::Metadata;
use crate::vfs::Identity;

/// The ordered isolation ladder a runtime builds one rung of.
///
/// `Hermetic` is the full ambient-denial posture: ambient time and
/// entropy denied at the WASI boundary, no ambient filesystem /
/// environment / network reachability, and no host import beyond the wire ABI's
/// `__kobako_upcall`. `Permissive` differs only in the ambient
/// authority it grants — live time and entropy at the WASI boundary. Ordering
/// follows strength (`Permissive < Hermetic`), so a floor check is a
/// plain comparison.
///
/// Deliberately not `#[non_exhaustive]`, unlike this crate's error
/// types. A wildcard arm here would let a runtime silently keep building
/// the posture it already knows while declaring it satisfied a rung it
/// never implemented; the compile error a new rung causes is what forces
/// each runtime to answer for it. The ordering is the same reason — a
/// rung inserted mid-ladder changes what every existing `>=` means, which
/// no wildcard could have absorbed either.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Profile {
    Permissive,
    Hermetic,
}

impl Profile {
    /// The inode number a guest on this posture sees for `identity`. A
    /// hermetic guest sees none, as a Filesystem's device number depends on
    /// how many were made before it in the process. Any other sees a hash of
    /// it, as wasmtime-wasi hands preview1 a metadata hash, so it can tell
    /// entries apart without reading a host's inode number; the hash is not a
    /// secret, since its key is fixed.
    pub fn guest_ino(self, identity: Option<Identity>) -> u64 {
        match (self, identity) {
            (Self::Hermetic, _) | (_, None) => 0,
            (Self::Permissive, Some(identity)) => {
                let mut hasher = DefaultHasher::new();
                identity.hash(&mut hasher);
                hasher.finish()
            }
        }
    }

    /// `metadata` as a guest on this posture sees it. A Filesystem records
    /// the host's real time whatever the invocation writing to it runs
    /// under, so one shared across invocations keeps one record; a hermetic
    /// guest sees every time at the Unix epoch, where its wall clock stands,
    /// so a time a write left behind is no clock it can read.
    pub fn mask_times(self, metadata: Metadata) -> Metadata {
        match self {
            Self::Permissive => metadata,
            Self::Hermetic => {
                let epoch = |time: Option<SystemTime>| time.map(|_| UNIX_EPOCH);
                metadata.with_times(
                    epoch(metadata.accessed),
                    epoch(metadata.modified),
                    epoch(metadata.changed),
                )
            }
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
