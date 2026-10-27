//! A Mount: the Grant handing a Filesystem to the guest at an absolute
//! guest path, with the access it allows there. The Mount decides each
//! open and each change itself, so a runtime asks it rather than reading
//! its access, and a Filesystem of a third party's checks nothing.

use std::fmt;
use std::sync::Arc;

use crate::filesystem::{Access, Filesystem, FilesystemError};

/// The Host App's decision for one path a guest opens or changes, made in
/// a Mount's access's place.
pub trait Policy: Send + Sync {
    /// Whether the guest may reach `path`, the absolute guest path, with
    /// `access`.
    fn allows(&self, path: &str, access: Access) -> bool;
}

/// Why a Mount refused a call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Denial {
    /// A change beneath a Mount allowing only reads.
    ReadOnly,
    /// The Mount's Policy said no.
    Refused,
}

/// A Filesystem at an absolute guest path. Its path bounds what a Policy
/// can allow either way.
#[derive(Clone)]
pub struct Mount {
    at: String,
    access: Access,
    filesystem: Arc<dyn Filesystem>,
    policy: Option<Arc<dyn Policy>>,
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

/// `path` beneath `root` with each name spelled as the directory holding
/// it stores it, reached one name at a time following no link; names past
/// the last one held stay as given.
fn stored_path(root: &Arc<dyn Filesystem>, path: &str) -> Result<String, FilesystemError> {
    let mut dir = Some(root.clone());
    let mut stored = Vec::new();
    let mut names = path.split('/').filter(|name| !name.is_empty()).peekable();
    while let Some(name) = names.next() {
        let spelled = match &dir {
            Some(dir) => dir.stored_name(name)?,
            None => name.to_string(),
        };
        dir = match dir {
            Some(dir) if names.peek().is_some() => match dir.open_dir_nofollow(&spelled) {
                Ok(next) => Some(next),
                Err(FilesystemError::NotFound | FilesystemError::NotADirectory) => None,
                Err(err) => return Err(err),
            },
            _ => None,
        };
        stored.push(spelled);
    }
    Ok(stored.join("/"))
}

impl Mount {
    /// A Mount of `filesystem` at `at`, refused unless `at` is an absolute
    /// guest path, which every path the Mount answers for is built on.
    pub fn new(
        at: impl Into<String>,
        filesystem: Arc<dyn Filesystem>,
        access: Access,
    ) -> Result<Self, MountPathError> {
        let at = at.into();
        if !is_guest_path(&at) {
            return Err(MountPathError(at));
        }
        Ok(Self {
            at,
            access,
            filesystem,
            policy: None,
        })
    }

    /// This Mount with `policy` deciding each open and each change in its
    /// access's place.
    pub fn with_policy(self, policy: Arc<dyn Policy>) -> Self {
        Self {
            policy: Some(policy),
            ..self
        }
    }

    /// The absolute guest path the Filesystem appears at.
    pub fn at(&self) -> &str {
        &self.at
    }

    /// The Filesystem an engine hands each call beneath this Mount.
    pub fn filesystem(&self) -> &Arc<dyn Filesystem> {
        &self.filesystem
    }

    /// Whether a change beneath this Mount may be allowed at all: it was made
    /// to be written, or a Policy decides each change in its access's place.
    /// The Policy itself is not asked.
    pub fn allows_writes(&self) -> bool {
        self.policy.is_some() || self.access == Access::ReadWrite
    }

    /// The absolute guest path `path`, relative to this Mount's root,
    /// stands for.
    pub fn guest_path(&self, path: &str) -> String {
        match (self.at.as_str(), path) {
            (at, "") => at.to_string(),
            ("/", path) => format!("/{path}"),
            (at, path) => format!("{at}/{path}"),
        }
    }

    /// Whether the guest may reach `path`, relative to this Mount's root,
    /// with `access`. A Policy judges the path as the Filesystem stores
    /// its names, so a host folding case or Unicode form cannot hand it
    /// one spelling of an entry it refuses by another; a Filesystem that
    /// cannot spell it refuses.
    pub fn check(&self, path: &str, access: Access) -> Result<(), Denial> {
        match &self.policy {
            Some(policy) => {
                let stored = stored_path(&self.filesystem, path).map_err(|_| Denial::Refused)?;
                if policy.allows(&self.guest_path(&stored), access) {
                    Ok(())
                } else {
                    Err(Denial::Refused)
                }
            }
            None if access == Access::ReadWrite && self.access == Access::Read => {
                Err(Denial::ReadOnly)
            }
            None => Ok(()),
        }
    }

    /// Whether a directory may move beneath this Mount. Under a Policy it
    /// may not: everything beneath it would take paths the Policy never
    /// judged, so a move is refused as one across devices and the guest
    /// copies entry by entry instead, as Landlock refuses such a move.
    pub fn moves_directories(&self) -> bool {
        self.policy.is_none()
    }

    /// Whether a path beneath this Mount may pass through a link the
    /// Filesystem holds. Under a Policy it may not: the link leads to a
    /// path the Policy never judged, so the call is refused as one past
    /// too many links.
    pub fn follows_links(&self) -> bool {
        self.policy.is_none()
    }

    /// This Mount as an Invocation reaches it; beneath a Policy, over the
    /// Filesystem that follows no link, so each call reaches the path the
    /// Policy judged.
    pub fn for_invocation(&self) -> Self {
        let filesystem = match self.policy {
            Some(_) => self
                .filesystem
                .for_policy()
                .unwrap_or_else(|| self.filesystem.clone()),
            None => self.filesystem.clone(),
        };
        Self {
            filesystem,
            ..self.clone()
        }
    }
}
