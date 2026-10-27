//! A directory beneath a Mount, as Linux's `struct path` pairs a mount
//! with a dentry.

use std::sync::Arc;

use super::Error;
use crate::filesystem::{DirEntry, Filesystem, FilesystemError, Metadata};
use crate::mount::Mount;

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

/// One directory on the way to another, and the name it was entered by.
pub(crate) struct Step {
    pub(crate) dir: Arc<dyn Filesystem>,
    pub(crate) name: String,
}

/// A directory beneath a Mount, as a directory descriptor holds one: the
/// Mount's root, or a directory opened beneath it.
#[derive(Clone)]
pub struct Path {
    mount: Arc<Mount>,
    /// The directory itself, which a call reaches wherever it moved.
    dir: Arc<dyn Filesystem>,
    /// Where it stood beneath the Mount's root when it was opened, which a
    /// Mount with a Policy reaches it by again.
    at: String,
    /// Its identity when it was opened, so one standing at `at` later is
    /// not taken for it.
    identity: Option<Identity>,
    /// Whether it is the Mount's root, which never moves.
    root: bool,
}

impl Path {
    /// The root of `mount`.
    pub fn root(mount: Mount) -> Self {
        Self {
            dir: mount.filesystem().clone(),
            mount: Arc::new(mount),
            at: String::new(),
            identity: None,
            root: true,
        }
    }

    pub(crate) fn opened(
        mount: Arc<Mount>,
        dir: Arc<dyn Filesystem>,
        at: String,
        identity: Option<Identity>,
    ) -> Self {
        Self {
            mount,
            dir,
            at,
            identity,
            root: false,
        }
    }

    /// The Mount the directory stands beneath.
    pub fn mount(&self) -> &Arc<Mount> {
        &self.mount
    }

    pub(crate) fn at(&self) -> &str {
        &self.at
    }

    /// What the directory reports of itself.
    pub fn metadata(&self) -> Result<Metadata, Error> {
        Ok(self.reach_dir()?.metadata("")?)
    }

    /// The entries of the directory, without `.` or `..`.
    pub fn entries(&self) -> Result<Vec<DirEntry>, Error> {
        Ok(self.reach_dir()?.entries()?)
    }

    fn reach_dir(&self) -> Result<Arc<dyn Filesystem>, Error> {
        let mut chain = self.reach()?;
        Ok(chain.pop().expect("a chain reaches its directory").dir)
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
            }]);
        }
        let mut chain = vec![Step {
            dir: self.mount.filesystem().clone(),
            name: String::new(),
        }];
        for name in self.at.split('/').filter(|name| !name.is_empty()) {
            let above = &chain.last().expect("a chain holds the root").dir;
            let dir = match above.open_dir_nofollow(name) {
                Ok(dir) => dir,
                Err(FilesystemError::NotFound | FilesystemError::NotADirectory) => {
                    return Err(FilesystemError::NotFound.into())
                }
                Err(err) => return Err(err.into()),
            };
            chain.push(Step {
                dir,
                name: name.to_string(),
            });
        }
        self.confirm(chain.last().expect("a chain holds the root").dir.as_ref())?;
        Ok(chain)
    }

    /// `found`, the directory reached at `at`, is still the one opened, as
    /// a POSIX descriptor never reaches another directory. A Mount's root
    /// never moves; any other directory opened with no identity cannot be
    /// told from another, so it is reached no more.
    fn confirm(&self, found: &dyn Filesystem) -> Result<(), Error> {
        if self.root {
            return Ok(());
        }
        let Some(identity) = self.identity else {
            return Err(FilesystemError::NotFound.into());
        };
        match found.metadata("") {
            Ok(found) if Identity::of(&found) == Some(identity) => Ok(()),
            Ok(_) | Err(FilesystemError::NotFound | FilesystemError::NotADirectory) => {
                Err(FilesystemError::NotFound.into())
            }
            Err(err) => Err(err.into()),
        }
    }
}
