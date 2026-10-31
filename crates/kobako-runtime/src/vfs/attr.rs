//! What a change sets of an entry, as Linux's `fs/attr.c` takes a
//! `struct iattr`.

use std::time::SystemTime;

/// What a change sets of an entry, as Linux's `struct iattr`; a field left
/// `None` is one `ia_valid` does not name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Iattr {
    /// The length to truncate or extend a file to, extending with zeros.
    pub size: Option<u64>,
    /// The access and modification times to set.
    pub atime: Option<SetTime>,
    pub mtime: Option<SetTime>,
    /// Whether the size comes from an open asking to truncate, as `ATTR_OPEN`
    /// says, so a Filesystem that truncated in `->open` has nothing left to do.
    pub open: bool,
}

impl Iattr {
    /// A change truncating or extending a file to `len`.
    pub fn size(len: u64) -> Self {
        Self {
            size: Some(len),
            atime: None,
            mtime: None,
            open: false,
        }
    }

    /// A change emptying a file an open asked to truncate.
    pub(crate) fn open_truncate() -> Self {
        Self {
            open: true,
            ..Self::size(0)
        }
    }

    /// A change setting the times named, leaving any other as it stands.
    pub fn times(atime: Option<SetTime>, mtime: Option<SetTime>) -> Self {
        Self {
            size: None,
            atime,
            mtime,
            open: false,
        }
    }
}

/// A time a change sets, as Linux's `ATTR_ATIME` names the moment it lands and
/// `ATTR_ATIME_SET` the time it carries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SetTime {
    Now,
    At(SystemTime),
}
