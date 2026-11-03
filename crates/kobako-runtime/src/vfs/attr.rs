//! What a change sets of an entry, as Linux's `fs/attr.c` takes a
//! `struct iattr`.

use std::time::SystemTime;

use super::dentry::Dentry;
use super::FilesystemError;
use crate::locking::RwSemaphoreGuard;

/// Apply `attr` to what `dentry` names, as Linux's `notify_change` reaches
/// `->setattr`, the moment it lands read once, as `current_time` is, so every
/// time it sets to now reads the same. The caller holds the entry's lock and
/// hands its guard over, as `notify_change` warns unless `inode_is_locked`;
/// the lock was reached through the entry, which a dentry never drops.
pub(crate) fn notify_change(
    dentry: &Dentry,
    held: &RwSemaphoreGuard,
    attr: &Iattr,
) -> Result<(), FilesystemError> {
    let inode = dentry
        .inode()
        .expect("a dentry whose entry is locked names it");
    debug_assert!(
        inode.is_locked_by(held),
        "notify_change takes the guard of the entry's own lock"
    );
    let attr = Iattr {
        ctime: Some(SystemTime::now()),
        ..*attr
    };
    inode.ops().setattr(dentry, &attr)
}

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
    /// The moment the change lands, which `notify_change` sets, as Linux's
    /// `ia_ctime`: what a time set to now reads, and the entry's change time.
    pub ctime: Option<SystemTime>,
}

impl Iattr {
    /// A change truncating or extending a file to `len`, moving its modification
    /// time to the moment it lands, as Linux's `do_truncate` names `ATTR_MTIME`
    /// with `ATTR_SIZE`.
    pub fn size(len: u64) -> Self {
        Self {
            size: Some(len),
            atime: None,
            mtime: Some(SetTime::Now),
            open: false,
            ctime: None,
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
            ctime: None,
        }
    }

    /// The time `set` names, now read as the moment this change lands.
    pub fn time_of(&self, set: SetTime) -> SystemTime {
        match set {
            SetTime::Now => self.ctime.unwrap_or_else(SystemTime::now),
            SetTime::At(time) => time,
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
