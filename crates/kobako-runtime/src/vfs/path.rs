//! A directory beneath a Mount, as Linux's `struct path` pairs a mount
//! with a dentry.

use std::sync::Arc;

use super::dentry::Dentry;
use super::seen::seen;
use super::Error;
use crate::filesystem::{DirEntry, Filesystem, FilesystemError, Metadata};
use crate::mount::Mount;
use crate::profile::Profile;

/// What a directory is, apart from where it stands: the device and inode
/// numbers its Filesystem names it by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Identity {
    dev: u64,
    ino: u64,
}

impl Identity {
    /// The identity `metadata` names, or `None` where its inode of zero
    /// reports none.
    pub(crate) fn of(metadata: &Metadata) -> Option<Self> {
        (metadata.ino != 0).then_some(Self {
            dev: metadata.dev,
            ino: metadata.ino,
        })
    }
}

/// One directory on the way to another, the name it was entered by, and
/// its dentry.
pub(crate) struct Step {
    pub(crate) dir: Arc<dyn Filesystem>,
    pub(crate) name: String,
    pub(crate) dentry: Arc<Dentry>,
}

/// A directory beneath a Mount, as a directory descriptor holds one: the
/// Mount's root, or a directory opened beneath it.
#[derive(Clone)]
pub struct Path {
    mount: Arc<Mount>,
    /// Where the directory stands in its SuperBlock, which a Mount with a
    /// Policy reaches it by again, and whether it was removed.
    dentry: Arc<Dentry>,
    /// The directory itself, which a call reaches wherever it moved.
    dir: Arc<dyn Filesystem>,
    /// Its identity when it was opened, so another standing where it
    /// stands later is not taken for it.
    identity: Option<Identity>,
    /// The isolation the invocation holding it runs under, which decides
    /// whether the times a Filesystem reports reach the guest.
    profile: Profile,
}

impl Path {
    /// The root of `mount`, for an invocation running under `profile`.
    pub fn root(mount: Mount, profile: Profile) -> Self {
        Self {
            dir: mount.super_block().root().clone(),
            dentry: mount.super_block().dentries().root(),
            mount: Arc::new(mount),
            identity: None,
            profile,
        }
    }

    /// The directory `dir`, opened beneath this one as `dentry`.
    pub(crate) fn opened(
        &self,
        dir: Arc<dyn Filesystem>,
        dentry: Arc<Dentry>,
        identity: Option<Identity>,
    ) -> Self {
        Self {
            mount: self.mount.clone(),
            dentry,
            dir,
            identity,
            profile: self.profile,
        }
    }

    pub(crate) fn profile(&self) -> Profile {
        self.profile
    }

    /// The Mount the directory stands beneath.
    pub fn mount(&self) -> &Arc<Mount> {
        &self.mount
    }

    pub(crate) fn dentry(&self) -> &Arc<Dentry> {
        &self.dentry
    }

    /// What the directory reports of itself.
    pub fn metadata(&self) -> Result<Metadata, Error> {
        Ok(seen(self.reach_dir()?.metadata()?, self.profile))
    }

    /// The entries of the directory, without `.` or `..`; none once it was
    /// removed, as Linux lists a dead directory.
    pub fn entries(&self) -> Result<Vec<DirEntry>, Error> {
        let dir = self.reach_dir()?;
        if self.dentry.is_dead() {
            return Ok(Vec::new());
        }
        Ok(dir.entries()?)
    }

    fn reach_dir(&self) -> Result<Arc<dyn Filesystem>, Error> {
        let mut chain = self.reach()?;
        Ok(chain.pop().expect("a chain reaches its directory").dir)
    }

    fn is_root(&self) -> bool {
        Arc::ptr_eq(&self.dentry, &self.mount.super_block().dentries().root())
    }

    /// Where a call through the directory starts, with each directory above
    /// it: the directory alone, or, beneath a Mount following no link,
    /// which is one with a Policy, every directory from the Mount's root
    /// down to it, found again so the Policy judges the path it stands at
    /// now.
    pub(crate) fn reach(&self) -> Result<Vec<Step>, Error> {
        if self.mount.follows_links() {
            return Ok(vec![Step {
                dir: self.dir.clone(),
                name: String::new(),
                dentry: self.dentry.clone(),
            }]);
        }
        let super_block = self.mount.super_block();
        let mut chain = vec![Step {
            dir: super_block.root().clone(),
            name: String::new(),
            dentry: super_block.dentries().root(),
        }];
        for name in self.dentry.names() {
            let above = chain.last().expect("a chain holds the root");
            let dir = match above.dir.open_dir_nofollow(&name) {
                Ok(dir) => dir,
                Err(FilesystemError::NotFound | FilesystemError::NotADirectory) => {
                    return Err(FilesystemError::NotFound.into())
                }
                Err(err) => return Err(err.into()),
            };
            let dentry = super_block.dentries().child(&above.dentry, &name);
            chain.push(Step { dir, name, dentry });
        }
        self.confirm(chain.last().expect("a chain holds the root").dir.as_ref())?;
        Ok(chain)
    }

    /// `found`, the directory reached where this one stands, is still the
    /// one opened, as a POSIX descriptor never reaches another directory. A
    /// Mount's root never moves; any other directory opened with no
    /// identity cannot be told from another, so it is reached no more.
    fn confirm(&self, found: &dyn Filesystem) -> Result<(), Error> {
        if self.is_root() {
            return Ok(());
        }
        let Some(identity) = self.identity else {
            return Err(FilesystemError::NotFound.into());
        };
        match found.metadata() {
            Ok(found) if Identity::of(&found) == Some(identity) => Ok(()),
            Ok(_) | Err(FilesystemError::NotFound | FilesystemError::NotADirectory) => {
                Err(FilesystemError::NotFound.into())
            }
            Err(err) => Err(err.into()),
        }
    }
}
