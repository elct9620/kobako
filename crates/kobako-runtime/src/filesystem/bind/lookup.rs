//! How a host directory spells a name it holds. A host that folds case or
//! Unicode form reaches one entry by many names, so the runtime keeping a
//! dentry per name as asked would hold one entry under several.
//!
//! macOS answers the stored name for any spelling. Elsewhere a listing
//! finds the entry by its inode, skipped on a Linux filesystem known to
//! match names exactly; without an inode, a spelling the listing lacks is
//! refused.

#[cfg(not(target_os = "macos"))]
use std::io;

use cap_std::fs::Dir;

use super::{to_error, FilesystemError};

/// The name `parent` stores for the entry `name` reaches, a link reached
/// as itself; `name` as given when nothing is there.
pub(super) fn lookup(parent: &Dir, name: &str) -> Result<String, FilesystemError> {
    #[cfg(target_os = "macos")]
    {
        macos::lookup(parent, name).map_err(to_error)
    }
    #[cfg(not(target_os = "macos"))]
    {
        #[cfg(target_os = "linux")]
        if linux::matches_exactly(parent) {
            return Ok(name.to_string());
        }
        lookup_by_listing(parent, name)
    }
}

/// The name a listing of `parent` holds for `name`: itself when listed,
/// else the entry sharing its inode. A spelling the listing lacks and no
/// inode ties to a listed name is refused as `EACCES`.
#[cfg(not(target_os = "macos"))]
fn lookup_by_listing(parent: &Dir, name: &str) -> Result<String, FilesystemError> {
    let found = match parent.symlink_metadata(name) {
        Ok(found) => found,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(name.to_string()),
        Err(err) => return Err(to_error(err)),
    };
    let mut names = Vec::new();
    for entry in parent.entries().map_err(to_error)? {
        let entry = entry.map_err(to_error)?;
        if entry.file_name() == name {
            return Ok(name.to_string());
        }
        names.push(entry);
    }
    stored_by_inode(names, &found)
}

/// The name among `names` sharing the inode of `found`.
#[cfg(all(unix, not(target_os = "macos")))]
fn stored_by_inode(
    names: Vec<cap_std::fs::DirEntry>,
    found: &cap_std::fs::Metadata,
) -> Result<String, FilesystemError> {
    use cap_std::fs::MetadataExt;
    use std::os::unix::fs::DirEntryExt;
    names
        .into_iter()
        .find(|entry| entry.ino() == found.ino())
        .ok_or(FilesystemError::PermissionDenied)?
        .file_name()
        .into_string()
        .map_err(|_| to_error(io::ErrorKind::InvalidData.into()))
}

/// Without an inode nothing ties a spelling to a listed name.
#[cfg(not(unix))]
fn stored_by_inode(
    _names: Vec<cap_std::fs::DirEntry>,
    _found: &cap_std::fs::Metadata,
) -> Result<String, FilesystemError> {
    Err(FilesystemError::PermissionDenied)
}

#[cfg(target_os = "macos")]
mod macos {
    use std::ffi::CString;
    use std::io;
    use std::os::fd::AsRawFd;

    use cap_std::fs::Dir;

    /// The reply `getattrlist(2)` writes for `ATTR_CMN_NAME`: its length,
    /// then a reference to the name stored behind it.
    #[repr(C, packed(4))]
    struct NameReply {
        length: u32,
        name: libc::attrreference_t,
        data: [u8; 1024],
    }

    pub(super) fn lookup(parent: &Dir, name: &str) -> io::Result<String> {
        let path = CString::new(name).map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
        let mut request = libc::attrlist {
            bitmapcount: libc::ATTR_BIT_MAP_COUNT,
            reserved: 0,
            commonattr: libc::ATTR_CMN_NAME,
            volattr: 0,
            dirattr: 0,
            fileattr: 0,
            forkattr: 0,
        };
        let mut reply = NameReply {
            length: 0,
            name: libc::attrreference_t {
                attr_dataoffset: 0,
                attr_length: 0,
            },
            data: [0; 1024],
        };
        // SAFETY: every pointer names a live, correctly sized buffer, and
        // the reply buffer's size bounds what the call writes.
        let answered = unsafe {
            libc::getattrlistat(
                parent.as_raw_fd(),
                path.as_ptr(),
                (&raw mut request).cast(),
                (&raw mut reply).cast(),
                size_of::<NameReply>(),
                libc::FSOPT_NOFOLLOW as libc::c_ulong,
            )
        };
        if answered != 0 {
            let err = io::Error::last_os_error();
            return match err.kind() {
                io::ErrorKind::NotFound => Ok(name.to_string()),
                _ => Err(err),
            };
        }
        // The reference counts from itself, and `data` follows it; the
        // length counts the terminating NUL.
        let start = usize::try_from(reply.name.attr_dataoffset)
            .ok()
            .and_then(|offset| offset.checked_sub(size_of::<libc::attrreference_t>()));
        let len = (reply.name.attr_length as usize).checked_sub(1);
        let stored = start
            .zip(len)
            .and_then(|(start, len)| reply.data.get(start..start.checked_add(len)?))
            .ok_or(io::ErrorKind::InvalidData)?;
        String::from_utf8(stored.to_vec()).map_err(|_| io::ErrorKind::InvalidData.into())
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use cap_std::fs::Dir;
    use rustix::fs::{fstatfs, ioctl_getflags, openat, Mode, OFlags};

    /// The 32-bit magic numbers of filesystems matching names byte for
    /// byte unless a directory is marked to fold case: ext4, Btrfs, tmpfs,
    /// overlayfs, and F2FS. XFS is left to the listing, as a volume made
    /// ASCII case-insensitive marks no directory.
    const EXACT: [u32; 5] = [0xEF53, 0x9123_683E, 0x0102_1994, 0x794C_7630, 0xF2F5_2010];

    /// The inode flag marking a directory whose names fold case, from
    /// Linux's `FS_CASEFOLD_FL`.
    const CASEFOLD: u32 = 0x4000_0000;

    /// cap-std holds `parent` for its path alone, which answers no flags,
    /// so they are asked of the same directory opened to read.
    pub(super) fn matches_exactly(parent: &Dir) -> bool {
        let exact = fstatfs(parent).is_ok_and(|stats| EXACT.contains(&(stats.f_type as u32)));
        exact
            && openat(
                parent,
                ".",
                OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
                Mode::empty(),
            )
            .and_then(|readable| ioctl_getflags(&readable))
            .is_ok_and(|flags| flags.bits() & CASEFOLD == 0)
    }

    #[cfg(test)]
    mod tests {
        use cap_std::ambient_authority;

        use super::super::lookup_by_listing;
        use super::{fstatfs, matches_exactly, Dir, EXACT};

        // A Directory opens its directories as cap-std does, for a path
        // alone, and a filesystem known to match exactly still spells
        // without listing. A host on another filesystem has nothing here
        // to witness.
        // @behavior FS-081
        #[test]
        fn a_directory_opened_for_its_path_alone_is_known_to_match_exactly() {
            let path = std::env::temp_dir().join(format!("kobako-exact-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).expect("the host directory must be created");
            let dir = Dir::open_ambient_dir(&path, ambient_authority())
                .expect("the host directory must open");
            let known = fstatfs(&dir).is_ok_and(|stats| EXACT.contains(&(stats.f_type as u32)));
            let exact = matches_exactly(&dir);
            let _ = std::fs::remove_dir_all(&path);
            if known {
                assert!(
                    exact,
                    "a directory cap-std opened on a filesystem known to match names exactly, \
                     asked through matches_exactly, must answer that it matches exactly"
                );
            }
        }

        // A Linux host whose filesystem is not known to match exactly,
        // XFS among them, spells by listing; the host here matches
        // exactly, so the listing must answer as the lookup would.
        // @behavior FS-081
        #[test]
        fn a_listing_spells_names_it_matches_exactly_as_given() {
            let path = std::env::temp_dir().join(format!("kobako-listed-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).expect("the host directory must be created");
            std::fs::write(path.join("file"), "x").expect("the host file must be written");
            let dir = Dir::open_ambient_dir(&path, ambient_authority())
                .expect("the host directory must open");
            let answers = (
                lookup_by_listing(&dir, "file").ok(),
                lookup_by_listing(&dir, "missing").ok(),
            );
            let _ = std::fs::remove_dir_all(&path);
            assert_eq!(
                answers,
                (Some("file".to_string()), Some("missing".to_string())),
                "a name the host holds, spelled by listing its directory, must come back as \
                 given, and a name it lacks as given too"
            );
        }
    }
}
