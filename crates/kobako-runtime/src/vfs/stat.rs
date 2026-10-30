//! Looking at what stands at a path, as Linux's `fstatat` and
//! `readlinkat` do.

use std::sync::Arc;

use super::cred::Cred;
use super::namei::{d_get_link, filename_lookup, names_directory};
use super::path::Path;
use super::times::mask_times;
use super::Error;
use crate::filesystem::{FileType, Metadata};

/// What `path` beneath `start` names, as Linux's `vfs_fstatat` answers: a
/// link at its end followed where `follow` asks, and otherwise reported as
/// itself unless a trailing `/` asks for the directory it leads to, as
/// `AT_SYMLINK_NOFOLLOW` leaves one.
pub fn vfs_fstatat(
    cred: &Arc<Cred>,
    start: &Path,
    path: &str,
    follow: bool,
) -> Result<Metadata, Error> {
    let follow = follow || names_directory(path);
    let mut nd = filename_lookup(cred, start, path, follow, false)?;
    // A link the walk did not follow at the end is one swapped in since,
    // so it is reported as itself rather than followed.
    nd.check_directory()?;
    Ok(hide_link(mask_times(nd.found()?, start.profile())))
}

/// `metadata` with no size for a link, so nothing of where it points
/// reaches the guest.
fn hide_link(metadata: Metadata) -> Metadata {
    match metadata.file_type {
        FileType::Symlink => Metadata { len: 0, ..metadata },
        _ => metadata,
    }
}

/// Where the link at `path` beneath `start` points, asked of the security
/// hook first, as `vfs_readlink` follows `security_inode_readlink`.
pub fn do_readlinkat(cred: &Arc<Cred>, start: &Path, path: &str) -> Result<String, Error> {
    let mut nd = filename_lookup(cred, start, path, false, false)?;
    nd.check_directory()?;
    cred.security_inode_readlink()?;
    Ok(d_get_link(&*nd.child()?)?)
}

/// Only a unix host lets a test make a link.
#[cfg(all(test, unix))]
mod tests {
    use std::fs;
    use std::sync::Arc;

    use crate::filesystem::{Access, Bind, FileType};
    use crate::mount::Mount;
    use crate::profile::Profile;
    use crate::vfs::{self, Filesystem, Path};

    // A host link reports the length of where it points, so only the VFS
    // keeps that length from the guest.
    // @behavior RT-173
    #[test]
    fn a_link_reports_no_size_however_its_filesystem_sizes_it() {
        let host = std::env::temp_dir().join(format!("kobako-link-size-{}", std::process::id()));
        let _ = fs::remove_dir_all(&host);
        fs::create_dir_all(&host).expect("the host directory must be created");
        std::os::unix::fs::symlink("a-target-path", host.join("link"))
            .expect("the link must be made");
        let bind = Bind::new(&host).expect("the host directory must open");
        let mount = Mount::new("/data", Filesystem::new(Arc::new(bind)), Access::Read)
            .expect("/data is a guest path");
        let found = vfs::vfs_fstatat(
            &Arc::default(),
            &Path::root(mount.mnt(), Profile::Permissive),
            "link",
            false,
        );
        let _ = fs::remove_dir_all(&host);
        assert_eq!(
            found.map(|metadata| (metadata.file_type, metadata.len)),
            Ok((FileType::Symlink, 0)),
            "a host link through vfs::vfs_fstatat without following must report a link of size zero"
        );
    }
}
