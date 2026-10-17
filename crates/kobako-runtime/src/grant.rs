//! A Grant: a host resource the Host App hands the runtime instead of
//! binding it by name. The Sandbox holds it and each Invocation receives it
//! afresh; the guest reaches it through its system interface.

use std::sync::Arc;

use crate::filesystem::Filesystem;

/// One resource handed over for an Invocation.
#[non_exhaustive]
pub enum Grant {
    /// A Filesystem appearing at a guest path. The runtime enforces the
    /// access, so the Filesystem need not.
    Filesystem {
        /// The absolute guest path the Filesystem appears at.
        at: String,
        access: Access,
        filesystem: Arc<dyn Filesystem>,
        /// Decides each open and each change in the access's place; the
        /// path bounds what it can allow either way.
        policy: Option<Arc<dyn Policy>>,
    },
}

impl Grant {
    pub fn kind(&self) -> GrantKind {
        match self {
            Self::Filesystem { .. } => GrantKind::Filesystem,
        }
    }
}

/// The kinds of Grant a runtime may support.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum GrantKind {
    Filesystem,
}

/// What the guest may do beneath a filesystem Grant.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    Read,
    ReadWrite,
}

/// The Host App's decision for one path a guest opens or changes, made in
/// a filesystem Grant's access's place.
pub trait Policy: Send + Sync {
    /// Whether the guest may reach `path`, the absolute guest path, with
    /// `access`.
    fn allows(&self, path: &str, access: Access) -> bool;
}
