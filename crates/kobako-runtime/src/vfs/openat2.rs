//! What an open asks for, as Linux's `openat2` takes an `open_how`.

use super::FilesystemError;

/// How `InodeOperations::open` opens a file.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct OpenOptions {
    pub read: bool,
    pub write: bool,
    /// Empty the file once it is open.
    pub truncate: bool,
}

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
    /// Land each write at the end, as `O_APPEND` does.
    pub append: bool,
    /// Ask not to wait, as `O_NONBLOCK` does.
    pub nonblock: bool,
    /// Make each write durable before it answers, as `O_SYNC` does.
    pub sync: bool,
}

/// Refuse flags no open can mean before any path is looked at, as Linux's
/// `build_open_flags` refuses `O_DIRECTORY` with `O_CREAT`, and answer whether
/// the walk follows a link at the end, which `O_CREAT` with `O_EXCL` never does.
pub(crate) fn build_open_flags(how: &OpenHow) -> Result<bool, FilesystemError> {
    match how.directory && how.create {
        true => Err(FilesystemError::InvalidInput),
        false => Ok(how.follow && !(how.create && how.exclusive)),
    }
}
