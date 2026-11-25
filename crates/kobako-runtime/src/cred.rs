//! Who a call is made for, as Linux's `struct cred`: the security modules
//! registered for the caller, the memory charged on its behalf, the profile it
//! sees entries through, and its mount and inode number namespaces. The hooks that ask its
//! modules live with the security framework, as `security/security.c` holds
//! them apart from `kernel/cred.c`.

use std::sync::Arc;

use crate::memcg::MemoryBudget;
use crate::profile::Profile;
use crate::security::SecurityOperations;
use crate::vfs::{InoNamespace, MntNamespace, VfsMount};

/// Who a call is made for, holding the security modules registered for it, as
/// `cred->security` holds a Landlock domain, and the memory budget the guest's
/// growth is charged to, which every Cred restricted from it shares, as a
/// task's memory cgroup does. Its profile decides what the
/// caller sees of an entry, as `cred->user_ns` decides the owner `cp_new_stat`
/// reports. Its mount namespace says which mount stands on which name, as a
/// task's `nsproxy` holds one beside its credentials, and its inode number
/// namespace which number a hermetic caller knows each entry by, as a PID
/// namespace sits there too.
#[derive(Clone)]
pub struct Cred {
    security: Vec<Arc<dyn SecurityOperations>>,
    memory: Arc<MemoryBudget>,
    profile: Profile,
    mnt_ns: Arc<MntNamespace>,
    ino_ns: Arc<InoNamespace>,
}

/// One no module is registered for, which this crate's tests make calls as.
#[cfg(test)]
impl Default for Cred {
    fn default() -> Self {
        Self {
            security: Vec::new(),
            memory: MemoryBudget::new(None),
            profile: Profile::Hermetic,
            mnt_ns: Arc::default(),
            ino_ns: Arc::default(),
        }
    }
}

impl Cred {
    /// One with `security` registered, its memory unbounded.
    pub(crate) fn new(security: Vec<Arc<dyn SecurityOperations>>) -> Arc<Self> {
        Self::within(security, MemoryBudget::new(None))
    }

    /// One with `security` registered, charging what is held on its behalf to
    /// `memory`, seeing entries as the hermetic profile does until told
    /// otherwise, and numbering them afresh, which every Cred restricted from
    /// it shares.
    pub(crate) fn within(
        security: Vec<Arc<dyn SecurityOperations>>,
        memory: Arc<MemoryBudget>,
    ) -> Arc<Self> {
        Arc::new(Self {
            security,
            memory,
            profile: Profile::Hermetic,
            mnt_ns: Arc::default(),
            ino_ns: Arc::default(),
        })
    }

    /// This Cred with `more` modules registered after its own, sharing the
    /// memory held on its behalf, as `landlock_restrict_self` adds a domain.
    pub(crate) fn with_modules(&self, more: Vec<Arc<dyn SecurityOperations>>) -> Arc<Self> {
        let mut security = self.security.clone();
        security.extend(more);
        Arc::new(Self {
            security,
            ..self.clone()
        })
    }

    /// This Cred with `mnt_ns` as its mount namespace, as a task that unshared
    /// its mounts holds a namespace of its own.
    pub(crate) fn with_mnt_ns(&self, mnt_ns: Arc<MntNamespace>) -> Arc<Self> {
        Arc::new(Self {
            mnt_ns,
            ..self.clone()
        })
    }

    /// The mount every guest path starts from, as a task's `fs->root`: the
    /// Mount at `/`, or the read-only root the runtime made beneath the others;
    /// none where no Mount was made.
    pub fn root(&self) -> Option<Arc<VfsMount>> {
        self.mnt_ns.root().cloned()
    }

    /// The mount a walk from the root of `mnt` stands on: the topmost of those
    /// stacked over that root, as Linux's `topmost_overmount` answers.
    pub fn topmost_overmount(&self, mnt: &Arc<VfsMount>) -> Arc<VfsMount> {
        self.mnt_ns.topmost_overmount(mnt)
    }

    /// This Cred seeing entries as `profile` does, as a task's credentials
    /// carry the user namespace its stat is reported in.
    pub fn with_profile(&self, profile: Profile) -> Arc<Self> {
        Arc::new(Self {
            profile,
            ..self.clone()
        })
    }

    /// The security modules registered for the caller, in the order they are
    /// asked.
    pub(crate) fn modules(&self) -> &[Arc<dyn SecurityOperations>] {
        &self.security
    }

    /// Which mount stands on which name for the caller.
    pub(crate) fn mnt_ns(&self) -> &MntNamespace {
        &self.mnt_ns
    }

    /// The isolation profile the caller runs under.
    pub fn profile(&self) -> Profile {
        self.profile
    }

    /// The budget a Wasm Driver charges the guest's growth to.
    pub fn memory_budget(&self) -> &Arc<MemoryBudget> {
        &self.memory
    }

    /// The inode numbers a hermetic caller knows its entries by.
    pub(crate) fn ino_ns(&self) -> &InoNamespace {
        &self.ino_ns
    }
}
