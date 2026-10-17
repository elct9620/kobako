//! Resolving a guest path beneath a directory descriptor, and the access
//! a filesystem Grant allows there.
//!
//! The runtime is the boundary: a path never climbs above its mount's
//! root, and what the guest may do is decided here, before the Filesystem
//! is asked, so a Filesystem of a third party's need check neither.

use kobako_runtime::grant::Access;

use super::errno::{Answer, Errno};
use crate::descriptors::{Directory, Mount};

/// `path`, beneath `dir`, as a path relative to the mount's root. An
/// absolute path or a `..` above the root is not the guest's to reach.
pub(crate) fn resolve(dir: &Directory, path: &str) -> Answer<String> {
    if path.starts_with('/') {
        return Err(Errno::Notcapable.into());
    }
    let mut components: Vec<&str> = dir.path.split('/').filter(|c| !c.is_empty()).collect();
    for component in path.split('/') {
        match component {
            "" | "." => {}
            ".." => {
                components.pop().ok_or(Errno::Notcapable)?;
            }
            component => components.push(component),
        }
    }
    Ok(components.join("/"))
}

/// Whether the guest may reach `path` with `access`. A Policy decides in
/// the Grant's access's place and answers a refusal as `ACCES`; without
/// one, a change beneath a read-only Grant is `ROFS`.
pub(crate) fn allow(mount: &Mount, path: &str, access: Access) -> Answer {
    match &mount.policy {
        Some(policy) if policy.allows(&mount.guest_path(path), access) => Ok(()),
        Some(_) => Err(Errno::Acces.into()),
        None if access == Access::ReadWrite && mount.access == Access::Read => {
            Err(Errno::Rofs.into())
        }
        None => Ok(()),
    }
}
