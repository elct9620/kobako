//! Looking at what stands at a path, as Linux's `fstatat` and
//! `readlinkat` do.

use std::hash::{BuildHasher, RandomState};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock};
use std::time::{SystemTime, UNIX_EPOCH};

use super::inode::Identity;
use super::namei::{filename_lookup, retry_estale};
use super::path::Start;
use super::Errno;
use crate::cred::Cred;
use crate::profile::Profile;

/// What a path names, as Linux's file types `FT_*` name each kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum FileType {
    RegFile,
    Dir,
    /// A link, as only a stat that does not follow it and a listing report one.
    Symlink,
    /// A FIFO a host holds, or a pipe of the runtime's own.
    Fifo,
    Sock,
    Blkdev,
    /// A character device a host holds, or a whiteout, as Linux makes one.
    Chrdev,
    /// What a platform names no kind for.
    Unknown,
}

/// Whether `kind` is a device, a FIFO, or a socket, as Linux's `special_file`
/// names what an open for writing takes no write access to its mount for.
pub(crate) fn special_file(kind: FileType) -> bool {
    matches!(
        kind,
        FileType::Chrdev | FileType::Blkdev | FileType::Fifo | FileType::Sock
    )
}

/// What a Filesystem reports of an entry, as Linux's `struct kstat`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Kstat {
    /// What the entry is, as the `S_IFMT` bits of `mode` say; the runtime
    /// keeps no permission bits, so the kind stands alone.
    pub file_type: FileType,
    /// The length in bytes, of a directory what its Filesystem reports.
    pub size: u64,
    /// The device and inode numbers naming what the path holds, as
    /// `st_dev` and `st_ino` do. An `ino` of zero reports none, so such a
    /// directory cannot be told from one made in its place.
    pub dev: u64,
    pub ino: u64,
    /// How many names the entry has, as `st_nlink` counts; one where the
    /// Filesystem keeps no count, as Linux starts every inode.
    pub nlink: u64,
    /// When the entry was last read, written, or changed, as `st_atime`,
    /// `st_mtime`, and `st_ctime` record; `None` where the Filesystem keeps
    /// no such time.
    pub atime: Option<SystemTime>,
    pub mtime: Option<SystemTime>,
    pub ctime: Option<SystemTime>,
}

impl Kstat {
    /// What an entry is and its size, reporting no identity or time.
    pub fn new(file_type: FileType, size: u64) -> Self {
        Self {
            file_type,
            size,
            dev: 0,
            ino: 0,
            nlink: 1,
            atime: None,
            mtime: None,
            ctime: None,
        }
    }

    /// The same, naming `dev` and `ino` as what the path holds.
    pub fn with_identity(self, dev: u64, ino: u64) -> Self {
        Self { dev, ino, ..self }
    }

    /// The same, counting `nlink` names.
    pub fn with_nlink(self, nlink: u64) -> Self {
        Self { nlink, ..self }
    }

    /// The same, recording when the entry was last read, written, and changed.
    pub fn with_times(
        self,
        atime: Option<SystemTime>,
        mtime: Option<SystemTime>,
        ctime: Option<SystemTime>,
    ) -> Self {
        Self {
            atime,
            mtime,
            ctime,
            ..self
        }
    }

    /// A device number for a Filesystem with none of its own, as each
    /// tmpfs mount takes one: no earlier call returned it, and its top bit,
    /// which no host's `st_dev` sets, keeps it apart from a host's.
    pub fn unique_device() -> u64 {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        (1 << 63) | NEXT.fetch_add(1, Ordering::Relaxed)
    }
}

/// What `path` beneath `start` names, as Linux's `vfs_fstatat` answers: a
/// link at its end followed where `follow` asks, and otherwise reported as
/// itself unless a trailing `/` asks for the directory it leads to, as
/// `AT_SYMLINK_NOFOLLOW` leaves one.
pub fn vfs_fstatat(
    cred: &Arc<Cred>,
    start: &impl Start,
    path: &str,
    follow: bool,
) -> Result<Kstat, Errno> {
    retry_estale(
        || filename_lookup(cred, start, path, follow),
        |nd| nd.getattr(),
    )
}

/// Where the link at `path` beneath `start` points, as Linux's `do_readlinkat`
/// asks the security hook only of a link, and answers anything else invalid.
/// It takes the name with `LOOKUP_EMPTY`, so an empty one names `start`
/// itself, and anything but a link there is not found. The caller's buffer is
/// the ABI's to judge, as only its syscall has one.
pub fn do_readlinkat(cred: &Arc<Cred>, start: &impl Start, path: &str) -> Result<String, Errno> {
    let walk = || match path.is_empty() {
        true => Ok((start.path_init()?.dentry().clone(), Errno::NotFound)),
        false => Ok((
            filename_lookup(cred, start, path, false)?.path()?,
            Errno::InvalidInput,
        )),
    };
    retry_estale(walk, |(target, refusal)| {
        if target.kind()? != FileType::Symlink {
            return Err(refusal);
        }
        cred.security_inode_readlink(&target)?;
        target.get_link()
    })
}

/// `stat` as the caller `cred` is made for sees it, whichever ABI a Wasm
/// Driver encodes it into, as Linux's `cp_new_stat` reports an owner in the
/// caller's user namespace. A Filesystem records the host's real time whatever
/// the invocation writing to it runs under, so one shared across invocations
/// keeps one record; a hermetic caller sees every time at the Unix epoch,
/// where its wall clock stands, so a time a write left behind is no clock it
/// can read. A link reports no size, so nothing of where it points reaches the
/// caller, and an entry is told apart only by what `ino_of` answers.
pub fn cp_new_stat(cred: &Cred, stat: Kstat) -> Kstat {
    let ino = ino_of(cred, Identity::of(&stat));
    let masked = match cred.profile() {
        Profile::Permissive => stat,
        Profile::Hermetic => {
            let epoch = |time: Option<SystemTime>| time.map(|_| UNIX_EPOCH);
            stat.with_times(epoch(stat.atime), epoch(stat.mtime), epoch(stat.ctime))
        }
    };
    Kstat {
        size: match masked.file_type {
            FileType::Symlink => 0,
            _ => masked.size,
        },
        ..masked.with_identity(0, ino)
    }
}

/// The inode number the caller `cred` is made for sees for `identity`, in a
/// stat or a listing. A hermetic caller sees none, as a Filesystem's device
/// number depends on how many were made before it in the process. Any other
/// sees a hash of it under a key the process draws once, so it can tell
/// entries apart and cannot work back to a host's inode number.
pub fn ino_of(cred: &Cred, identity: Option<Identity>) -> u64 {
    static KEY: LazyLock<RandomState> = LazyLock::new(RandomState::new);
    match (cred.profile(), identity) {
        (Profile::Hermetic, _) | (_, None) => 0,
        (Profile::Permissive, Some(identity)) => KEY.hash_one(identity),
    }
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{cp_new_stat, ino_of, FileType, Kstat};
    use crate::cred::Cred;
    use crate::profile::Profile;
    use crate::vfs::Identity;

    fn seen_by(profile: Profile, stat: Kstat) -> Kstat {
        cp_new_stat(&Cred::default().with_profile(profile), stat)
    }

    // The Wasm Driver only encodes this, so another one shows the same, and a
    // guest reads a link's size and a time here or nowhere.
    // @behavior RT-173 RT-183
    #[test]
    fn a_guest_sees_no_size_for_a_link_and_only_what_its_profile_allows() {
        let reported = Kstat::new(FileType::Symlink, 42)
            .with_identity(7, 9)
            .with_times(Some(SystemTime::now()), None, None);
        let (hermetic, permissive) = (
            seen_by(Profile::Hermetic, reported),
            seen_by(Profile::Permissive, reported),
        );
        assert_eq!(
            (hermetic.size, permissive.size, hermetic.ino, hermetic.atime),
            (0, 0, 0, Some(UNIX_EPOCH)),
            "a link's stat through cp_new_stat must report no size on any profile, and on the \
             hermetic profile no inode number and every time at the Unix epoch"
        );
        assert!(
            permissive.ino != 0 && permissive.ino != 9 && permissive.atime == reported.atime,
            "a stat through cp_new_stat on the permissive profile must hash the inode number and \
             keep the time"
        );
    }

    // A device number depends on how many Filesystems the process made before,
    // so the inode number a guest reads folds it in and the device stays out.
    // @behavior RT-283
    #[test]
    fn a_guest_sees_one_device_for_every_entry() {
        let reported = Kstat::new(FileType::RegFile, 1).with_identity(7, 9);
        assert_eq!(
            [Profile::Hermetic, Profile::Permissive].map(|profile| seen_by(profile, reported).dev),
            [0, 0],
            "the stat through cp_new_stat of an entry a Filesystem reports on device 7 must \
             report device 0 on either profile"
        );
    }

    // An unkeyed hash answers what anyone can compute from a host's numbers,
    // so the hash a guest reads must differ from it.
    // @behavior RT-194
    #[test]
    fn a_guest_cannot_compute_the_inode_number_it_reads() {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let reported = Kstat::new(FileType::RegFile, 0).with_identity(7, 9);
        let identity = Identity::of(&reported);
        let mut unkeyed = DefaultHasher::new();
        identity
            .expect("an inode number names an identity")
            .hash(&mut unkeyed);
        let permissive = Cred::default().with_profile(Profile::Permissive);
        let seen = ino_of(&permissive, identity);
        assert_eq!(
            seen,
            ino_of(&permissive, identity),
            "ino_of one identity must answer one number within a process"
        );
        assert_ne!(
            seen,
            unkeyed.finish(),
            "ino_of on the permissive profile must not answer a hash anyone can compute from the \
             device and inode numbers"
        );
    }

    // A guest reads the count on any ABI, so the count starts in the runtime
    // rather than in what a Wasm Driver encodes.
    // @behavior RT-193
    #[test]
    fn an_entry_whose_filesystem_keeps_no_count_is_named_once() {
        let reported = Kstat::new(FileType::RegFile, 0);
        assert_eq!(
            (
                seen_by(Profile::Hermetic, reported).nlink,
                seen_by(Profile::Permissive, reported).nlink,
                seen_by(Profile::Permissive, reported.with_nlink(3)).nlink,
            ),
            (1, 1, 3),
            "the stat through cp_new_stat of an entry reporting no count must show one name on \
             each profile, and of one reporting three must show three"
        );
    }
}
