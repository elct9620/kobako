//! What an open asks for, as Linux's `openat2` takes an `open_how`.

use super::inode::Mask;
use super::FilesystemError;

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
    /// Make what each write needs read back durable before it answers, as
    /// `O_DSYNC` does.
    pub dsync: bool,
    /// Make everything each write changed durable before it answers, as
    /// `O_SYNC` does.
    pub sync: bool,
}

/// What an open was found to ask, as Linux's `struct open_flags` holds what
/// `build_open_flags` made of an `open_how`, which alone makes one, so what
/// it asks of the entry always matches the flags. A Filesystem reads
/// `open_flag`, as `->atomic_open` takes it and `->open` reads `f_flags`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OpenFlags {
    open_flag: OpenHow,
    acc_mode: Mask,
    lookup_follow: bool,
    lookup_directory: bool,
}

impl OpenFlags {
    /// The flags the open was made with, as `open_flag` keeps them.
    pub fn open_flag(&self) -> &OpenHow {
        &self.open_flag
    }

    /// What the open asks of what it opens, as `acc_mode` carries it;
    /// `may_open` adds `MAY_OPEN` when it asks.
    pub(crate) fn acc_mode(&self) -> Mask {
        self.acc_mode
    }

    /// Whether the walk follows a link at the end, as `LOOKUP_FOLLOW` in
    /// `lookup_flags` asks.
    pub(crate) fn lookup_follow(&self) -> bool {
        self.lookup_follow
    }

    /// Whether the walk must end on a directory, as `LOOKUP_DIRECTORY` in
    /// `lookup_flags` asks.
    pub(crate) fn lookup_directory(&self) -> bool {
        self.lookup_directory
    }
}

/// Refuse flags no open can mean before any path is looked at, as Linux's
/// `build_open_flags` refuses `O_DIRECTORY` with `O_CREAT`, and make what the
/// open asks: `O_CREAT` with `O_EXCL` never follows a link at the end, and
/// `O_SYNC` carries `O_DSYNC`, as Linux implements it as both. An open asking
/// neither to read nor to write stands only for its path, as `O_PATH` keeps
/// only `O_PATH_FLAGS` and asks nothing of the entry. An ABI asks it once,
/// before it reserves a descriptor, and hands what it made to `do_file_open`,
/// as `do_sys_openat2` does.
pub fn build_open_flags(how: &OpenHow) -> Result<OpenFlags, FilesystemError> {
    if !how.read && !how.write {
        return Ok(OpenFlags {
            open_flag: OpenHow {
                directory: how.directory,
                follow: how.follow,
                ..OpenHow::default()
            },
            acc_mode: Mask::NONE,
            lookup_follow: how.follow,
            lookup_directory: how.directory,
        });
    }
    if how.directory && how.create {
        return Err(FilesystemError::InvalidInput);
    }
    let mut acc_mode = Mask::NONE;
    if how.read {
        acc_mode = acc_mode | Mask::READ;
    }
    if how.write || how.truncate {
        acc_mode = acc_mode | Mask::WRITE;
    }
    if how.append {
        acc_mode = acc_mode | Mask::APPEND;
    }
    Ok(OpenFlags {
        open_flag: OpenHow {
            dsync: how.dsync || how.sync,
            ..*how
        },
        acc_mode,
        lookup_follow: how.follow && !(how.create && how.exclusive),
        lookup_directory: how.directory,
    })
}
