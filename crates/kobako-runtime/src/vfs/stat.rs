//! Looking at what stands at a path, as Linux's `fstatat` and
//! `readlinkat` do.

use std::sync::Arc;

use super::cred::Cred;
use super::namei::{d_get_link, filename_lookup, names_directory};
use super::path::Path;
use super::Error;
use crate::filesystem::Metadata;

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
    Ok(nd.found()?)
}

/// Where the link at `path` beneath `start` points, asked of the security
/// hook first, as `vfs_readlink` follows `security_inode_readlink`.
pub fn do_readlinkat(cred: &Arc<Cred>, start: &Path, path: &str) -> Result<String, Error> {
    let mut nd = filename_lookup(cred, start, path, false, false)?;
    nd.check_directory()?;
    cred.security_inode_readlink()?;
    Ok(d_get_link(&*nd.child()?)?)
}
