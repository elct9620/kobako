//! A Mount: the Grant handing a Filesystem to the guest at an absolute
//! guest path, with the access it allows there. The Mount decides each
//! open and each change itself, so a runtime asks it rather than reading
//! its access, and a Filesystem of a third party's checks nothing.

use std::fmt;
use std::sync::Arc;

use crate::vfs::{Filesystem, VfsMount};

pub use crate::vfs::{Access, Denial};

/// The Host App's decision for one path a guest opens or changes, made after
/// a Mount's access allows it, so it can only refuse more.
pub trait Policy: Send + Sync {
    /// Whether the guest may reach `path`, the absolute guest path, with
    /// `access`.
    fn allows(&self, path: &str, access: Access) -> bool;
}

/// A Filesystem instance at an absolute guest path. Its path bounds what a
/// Policy can allow either way.
#[derive(Clone)]
pub struct Mount {
    at: String,
    access: Access,
    filesystem: Arc<Filesystem>,
    policy: Option<Arc<dyn Policy>>,
    mnt: Arc<VfsMount>,
}

/// A path no Mount can stand at: one that is not an absolute guest path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MountPathError(String);

impl fmt::Display for MountPathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "mount path must be an absolute guest path, got {:?}",
            self.0
        )
    }
}

impl std::error::Error for MountPathError {}

/// An absolute guest path has no empty, `.`, or `..` component; the root
/// is `/` alone.
fn is_guest_path(at: &str) -> bool {
    at == "/"
        || at.strip_prefix('/').is_some_and(|rest| {
            rest.split('/')
                .all(|part| !part.is_empty() && part != "." && part != "..")
        })
}

impl Mount {
    /// A Mount of `filesystem` at `at`, refused unless `at` is an absolute
    /// guest path, which every path the Mount answers for is built on.
    pub fn new(
        at: impl Into<String>,
        filesystem: Arc<Filesystem>,
        access: Access,
    ) -> Result<Self, MountPathError> {
        let at = at.into();
        if !is_guest_path(&at) {
            return Err(MountPathError(at));
        }
        Ok(Self {
            at,
            access,
            mnt: VfsMount::new(filesystem.clone(), access == Access::Read),
            filesystem,
            policy: None,
        })
    }

    /// This Mount with `policy` also judging each open and each change its
    /// access allows; it can only refuse more, as an LSM can.
    pub fn with_policy(self, policy: Arc<dyn Policy>) -> Self {
        Self {
            policy: Some(policy),
            ..self
        }
    }

    /// The Filesystem as the VFS walks it.
    pub fn mnt(&self) -> &Arc<VfsMount> {
        &self.mnt
    }

    /// The Policy deciding beneath this Mount, once one was given.
    pub(crate) fn policy(&self) -> Option<&Arc<dyn Policy>> {
        self.policy.as_ref()
    }

    /// The absolute guest path the Filesystem appears at.
    pub fn at(&self) -> &str {
        &self.at
    }

    /// The Filesystem instance this Mount places.
    pub fn filesystem(&self) -> &Arc<Filesystem> {
        &self.filesystem
    }

    /// Whether a change beneath this Mount may be allowed at all. A Policy is
    /// not asked, and can only refuse more.
    pub fn allows_writes(&self) -> bool {
        self.access == Access::ReadWrite
    }
}
