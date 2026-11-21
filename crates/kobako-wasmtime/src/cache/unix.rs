//! The cache directory on Unix, reached through one descriptor: every step
//! after the trust check goes through it, so a path swapped after the check
//! reaches nothing.

use std::ffi::{OsStr, OsString};
use std::fs;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rustix::fd::OwnedFd;
use rustix::fs::{
    fstat, fsync, open, openat, renameat, statat, unlinkat, AtFlags, Dir, Mode, OFlags,
};

pub(super) struct PrivateDir(OwnedFd);

impl PrivateDir {
    /// Create `dir` owner-only (`0700`) so no other local user can plant an
    /// artifact the unsafe deserialize would trust.
    pub(super) fn create(dir: &Path) -> io::Result<()> {
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)
    }

    /// `dir`, once it upholds the trust the unsafe deserialize relies on: a
    /// directory rather than a link in its place, owned by the current
    /// effective user and writable by no one else.
    pub(super) fn open(dir: &Path) -> Option<Self> {
        let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
        let fd = open(dir, flags, Mode::empty()).ok()?;
        let stat = fstat(&fd).ok()?;
        let owned = stat.st_uid == rustix::process::geteuid().as_raw();
        (owned && stat.st_mode & 0o022 == 0).then_some(Self(fd))
    }

    /// The entry `name`, opened to read without following a link.
    pub(super) fn open_entry(&self, name: &OsStr) -> Option<fs::File> {
        let flags = OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
        let fd = openat(&self.0, name, flags, Mode::empty()).ok()?;
        Some(fs::File::from(fd))
    }

    /// A temp file no other writer shares: two threads of one process storing
    /// the same artifact at once would otherwise write into one file, and a
    /// reader could load the interleaved bytes the unsafe deserialize trusts.
    pub(super) fn create_temp(&self, name: &OsStr) -> Option<(PathBuf, fs::File)> {
        static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);
        let serial = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let tmp = Path::new(name).with_extension(format!("tmp{}.{serial}", std::process::id()));
        let flags =
            OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC;
        let fd = openat(&self.0, &tmp, flags, Mode::RUSR | Mode::WUSR).ok()?;
        Some((tmp, fs::File::from(fd)))
    }

    /// Move `tmp` over `name`, syncing the directory so the new name
    /// survives a crash; whether it moved.
    pub(super) fn rename(&self, tmp: &Path, name: &OsStr) -> bool {
        let moved = renameat(&self.0, tmp, &self.0, name).is_ok();
        if moved {
            let _ = fsync(&self.0);
        }
        moved
    }

    /// Each entry's name and when it was last modified, read without
    /// following a link; one whose time cannot be read is left out.
    pub(super) fn entries(&self) -> Vec<(OsString, SystemTime)> {
        let Ok(mut entries) = Dir::read_from(&self.0) else {
            return Vec::new();
        };
        let mut found = Vec::new();
        while let Some(Ok(entry)) = entries.read() {
            let name = entry.file_name();
            let modified = statat(&self.0, name, AtFlags::SYMLINK_NOFOLLOW)
                .ok()
                .and_then(|stat| u64::try_from(stat.st_mtime).ok())
                .map(|secs| UNIX_EPOCH + Duration::from_secs(secs));
            if let Some(modified) = modified {
                found.push((OsStr::from_bytes(name.to_bytes()).to_owned(), modified));
            }
        }
        found
    }

    pub(super) fn remove(&self, name: &OsStr) {
        let _ = unlinkat(&self.0, name, AtFlags::empty());
    }
}
