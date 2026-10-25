//! How a host directory spells a name it holds. A host that folds case or
//! Unicode form reaches one entry by many names, so a Policy judging the
//! name as asked would see a spelling the host never stored.
//!
//! macOS answers the stored name for any spelling. Elsewhere a listing
//! finds the entry by its inode, skipped on a Linux filesystem known to
//! match names exactly; without an inode, a spelling the listing lacks is
//! refused.

use std::io;

use cap_std::fs::Dir;

/// The name `parent` stores for the entry `name` reaches, a link reached
/// as itself, or `None` when nothing is there.
pub(super) fn stored_name(parent: &Dir, name: &str) -> io::Result<Option<String>> {
    #[cfg(target_os = "macos")]
    {
        macos::stored_name(parent, name)
    }
    #[cfg(not(target_os = "macos"))]
    {
        #[cfg(target_os = "linux")]
        if linux::matches_exactly(parent) {
            return match parent.symlink_metadata(name) {
                Ok(_) => Ok(Some(name.to_string())),
                Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
                Err(err) => Err(err),
            };
        }
        listed(parent, name)
    }
}

/// The name a listing of `parent` holds for `name`: itself when listed,
/// else the entry sharing its inode.
#[cfg(not(target_os = "macos"))]
fn listed(parent: &Dir, name: &str) -> io::Result<Option<String>> {
    let found = match parent.symlink_metadata(name) {
        Ok(found) => found,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err),
    };
    let mut names = Vec::new();
    for entry in parent.entries()? {
        let entry = entry?;
        if entry.file_name() == name {
            return Ok(Some(name.to_string()));
        }
        names.push(entry);
    }
    #[cfg(unix)]
    {
        use cap_std::fs::MetadataExt;
        use std::os::unix::fs::DirEntryExt;
        for entry in names {
            if entry.ino() == found.ino() {
                return entry
                    .file_name()
                    .into_string()
                    .map(Some)
                    .map_err(|_| io::ErrorKind::InvalidData.into());
            }
        }
    }
    #[cfg(not(unix))]
    let _ = (found, names);
    Err(io::ErrorKind::PermissionDenied.into())
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

    pub(super) fn stored_name(parent: &Dir, name: &str) -> io::Result<Option<String>> {
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
                io::ErrorKind::NotFound => Ok(None),
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
        String::from_utf8(stored.to_vec())
            .map(Some)
            .map_err(|_| io::ErrorKind::InvalidData.into())
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use cap_std::fs::Dir;
    use rustix::fs::{fstatfs, ioctl_getflags};

    /// The 32-bit magic numbers of filesystems matching names byte for
    /// byte unless a directory is marked to fold case: ext4, Btrfs, tmpfs,
    /// overlayfs, and F2FS. XFS is left to the listing, as a volume made
    /// ASCII case-insensitive marks no directory.
    const EXACT: [u32; 5] = [0xEF53, 0x9123_683E, 0x0102_1994, 0x794C_7630, 0xF2F5_2010];

    /// The inode flag marking a directory whose names fold case, from
    /// Linux's `FS_CASEFOLD_FL`.
    const CASEFOLD: u32 = 0x4000_0000;

    pub(super) fn matches_exactly(parent: &Dir) -> bool {
        let exact = fstatfs(parent).is_ok_and(|stats| EXACT.contains(&(stats.f_type as u32)));
        exact && ioctl_getflags(parent).is_ok_and(|flags| flags.bits() & CASEFOLD == 0)
    }

    #[cfg(test)]
    mod tests {
        use cap_std::ambient_authority;

        use super::super::listed;
        use super::Dir;

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
            let answers = (listed(&dir, "file").ok(), listed(&dir, "missing").ok());
            let _ = std::fs::remove_dir_all(&path);
            assert_eq!(
                answers,
                (Some(Some("file".to_string())), Some(None)),
                "a name the host holds, spelled by listing its directory, must come back as \
                 given, and a name it lacks as none"
            );
        }
    }
}
