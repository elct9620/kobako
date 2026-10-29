//! What an open asks for and what it reaches, as Linux's `openat2` takes
//! an `open_how` and `vfs_open` holds what the walk opened.

use super::dentry::Identity;
use super::namei::{d_can_lookup, down_read, Nameidata};
use super::path::Path;
use super::Error;
use crate::filesystem::File;

/// How to open a path, as Linux's `struct open_how` carries `openat2`'s
/// flags.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct OpenHow {
    pub read: bool,
    pub write: bool,
    /// Create a file where nothing stands, as `O_CREAT` does.
    pub create: bool,
    /// Refuse what already stands there, as `O_EXCL` does.
    pub exclusive: bool,
    /// Empty the file once it is open, as `O_TRUNC` does.
    pub truncate: bool,
    /// Refuse anything but a directory, as `O_DIRECTORY` does.
    pub directory: bool,
    /// Follow a link at the end of the path.
    pub follow: bool,
}

/// What an open reached, as POSIX calls the open file description a
/// descriptor refers to.
pub enum FileDescription {
    Directory(Path),
    File(Box<dyn File>),
}

/// The directory `nd` ends at, opened now, as Linux's `vfs_open` holds
/// what it opened, so the descriptor follows it wherever it moves.
pub(crate) fn vfs_open(
    start: &Path,
    nd: Nameidata,
    identity: Option<Identity>,
) -> Result<FileDescription, Error> {
    let (dir, dentry) = match nd.name.as_str() {
        "" => (nd.base().clone(), nd.dentry().clone()),
        _ => {
            let _held = down_read(&nd.mount, nd.dentry())?;
            let dentry = nd.child()?;
            let dir = nd.base().open_dir(&dentry)?;
            d_can_lookup(&dir, &dentry)?;
            (dir, dentry)
        }
    };
    dentry.d_instantiate(identity);
    Ok(FileDescription::Directory(
        start.with_directory(dir, dentry),
    ))
}
