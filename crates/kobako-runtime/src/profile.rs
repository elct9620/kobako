//! Isolation profile — the posture a frontend requests and a runtime
//! builds and declares.
//!
//! A rung on an ordered ladder: the host application requests the
//! posture it wants, the runtime builds it and declares the posture it
//! actually built, and the frontend refuses a declaration below the
//! request — so the request is also the floor.

use std::hash::{BuildHasher, RandomState};
use std::sync::LazyLock;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::filesystem::{FileType, Metadata};
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
    /// it under a key the process draws once, so it can tell entries apart
    /// and cannot work back to a host's inode number.
    pub fn guest_ino(self, identity: Option<Identity>) -> u64 {
        static KEY: LazyLock<RandomState> = LazyLock::new(RandomState::new);
        match (self, identity) {
            (Self::Hermetic, _) | (_, None) => 0,
            (Self::Permissive, Some(identity)) => KEY.hash_one(identity),
        }
    }

    /// `metadata` as a guest on this posture sees it, whichever ABI a Driver
    /// encodes it into. A Filesystem records the host's real time whatever
    /// the invocation writing to it runs under, so one shared across
    /// invocations keeps one record; a hermetic guest sees every time at the
    /// Unix epoch, where its wall clock stands, so a time a write left behind
    /// is no clock it can read. A link reports no size, so nothing of where it
    /// points reaches the guest, and an entry is told apart only by what
    /// `guest_ino` answers.
    pub fn guest_metadata(self, metadata: Metadata) -> Metadata {
        let ino = self.guest_ino(Identity::of(&metadata));
        let masked = match self {
            Self::Permissive => metadata,
            Self::Hermetic => {
                let epoch = |time: Option<SystemTime>| time.map(|_| UNIX_EPOCH);
                metadata.with_times(
                    epoch(metadata.accessed),
                    epoch(metadata.modified),
                    epoch(metadata.changed),
                )
            }
        };
        Metadata {
            len: match masked.file_type {
                FileType::Symlink => 0,
                _ => masked.len,
            },
            ..masked.with_identity(0, ino)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The Driver only encodes this, so another Driver shows the same, and a
    // guest reads a link's size and a time here or nowhere.
    // @behavior RT-173 RT-183
    #[test]
    fn a_guest_sees_no_size_for_a_link_and_only_what_its_posture_allows() {
        let reported = Metadata::new(FileType::Symlink, 42)
            .with_identity(7, 9)
            .with_times(Some(SystemTime::now()), None, None);
        let (hermetic, permissive) = (
            Profile::Hermetic.guest_metadata(reported),
            Profile::Permissive.guest_metadata(reported),
        );
        assert_eq!(
            (
                hermetic.len,
                permissive.len,
                hermetic.ino,
                hermetic.accessed
            ),
            (0, 0, 0, Some(UNIX_EPOCH)),
            "Profile::guest_metadata of a link must report no size on any posture, and at the \
             hermetic rung no inode number and every time at the Unix epoch"
        );
        assert!(
            permissive.ino != 0 && permissive.ino != 9 && permissive.accessed == reported.accessed,
            "Profile::guest_metadata at the permissive rung must hash the inode number and keep \
             the time"
        );
    }

    // An unkeyed hash answers what anyone can compute from a host's numbers,
    // so the hash a guest reads must differ from it.
    // @behavior RT-194
    #[test]
    fn a_guest_cannot_compute_the_inode_number_it_reads() {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let reported = Metadata::new(FileType::File, 0).with_identity(7, 9);
        let identity = Identity::of(&reported);
        let mut unkeyed = DefaultHasher::new();
        identity
            .expect("an inode number names an identity")
            .hash(&mut unkeyed);
        let seen = Profile::Permissive.guest_ino(identity);
        assert_eq!(
            seen,
            Profile::Permissive.guest_ino(identity),
            "Profile::guest_ino of one identity must answer one number within a process"
        );
        assert_ne!(
            seen,
            unkeyed.finish(),
            "Profile::guest_ino at the permissive rung must not answer a hash anyone can compute \
             from the device and inode numbers"
        );
    }

    // A guest reads the count on any ABI, so the count starts in the runtime
    // rather than in what a Driver encodes.
    // @behavior RT-193
    #[test]
    fn an_entry_whose_filesystem_keeps_no_count_is_named_once() {
        let reported = Metadata::new(FileType::File, 0);
        assert_eq!(
            (
                Profile::Hermetic.guest_metadata(reported).nlink,
                Profile::Permissive.guest_metadata(reported).nlink,
                Profile::Permissive
                    .guest_metadata(reported.with_nlink(3))
                    .nlink,
            ),
            (1, 1, 3),
            "Profile::guest_metadata of an entry reporting no count must show one name on each \
             posture, and of one reporting three must show three"
        );
    }

    // @behavior RT-014
    #[test]
    fn a_declaration_satisfies_any_floor_at_or_below_it() {
        assert!(Profile::Hermetic >= Profile::Hermetic);
        assert!(Profile::Hermetic >= Profile::Permissive);
        assert!(Profile::Permissive < Profile::Hermetic);
    }
}
