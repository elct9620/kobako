//! Resolving a guest path beneath a directory descriptor.
//!
//! The runtime is the boundary: a path never climbs above its Mount's
//! root, and the Mount is asked what the guest may do there before the
//! Filesystem is, so a Filesystem of a third party's need check neither.

use super::errno::{Answer, Errno};
use crate::descriptors::Directory;

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
