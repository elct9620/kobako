//! A Mount: the Grant handing a Filesystem to the guest at an absolute
//! guest path, with the access it allows there. The Mount decides each
//! open and each change itself, so a runtime asks it rather than reading
//! its access, and a Filesystem of a third party's checks nothing.

use std::sync::Arc;

use crate::filesystem::Filesystem;

/// What the guest may do beneath a Mount.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    Read,
    ReadWrite,
}

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

impl Mount {
    pub fn new(at: impl Into<String>, filesystem: Arc<dyn Filesystem>, access: Access) -> Self {
        Self {
            at: at.into(),
            access,
            filesystem,
            policy: None,
        }
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

    pub fn filesystem(&self) -> &Arc<dyn Filesystem> {
        &self.filesystem
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
    /// with `access`.
    pub fn check(&self, path: &str, access: Access) -> Result<(), Denial> {
        match &self.policy {
            Some(policy) if policy.allows(&self.guest_path(path), access) => Ok(()),
            Some(_) => Err(Denial::Refused),
            None if access == Access::ReadWrite && self.access == Access::Read => {
                Err(Denial::ReadOnly)
            }
            None => Ok(()),
        }
    }

    /// This Mount as a new Invocation starts from it, over the Filesystem
    /// that Invocation receives.
    pub fn for_invocation(&self) -> Self {
        Self {
            filesystem: self
                .filesystem
                .for_invocation()
                .unwrap_or_else(|| self.filesystem.clone()),
            ..self.clone()
        }
    }
}
