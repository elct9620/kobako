//! The Grants a Sandbox hands each invocation. They seal with the rest of
//! the Catalog, and each invocation receives them afresh.

use kobako_runtime::grant::Grant;
use kobako_runtime::mount::Mount;

use crate::error::Error;

fn to_mount_path(grant: &Grant) -> Option<&str> {
    match grant {
        Grant::Mount(mount) => Some(mount.at()),
        _ => None,
    }
}

#[derive(Default)]
pub(crate) struct Grants {
    entries: Vec<Grant>,
}

impl Grants {
    /// A path already mounted is refused, as Docker refuses a duplicate
    /// mount point; one beneath another Mount is not.
    pub(crate) fn mount(&mut self, mount: Mount) -> Result<(), Error> {
        let at = mount.at();
        if self.is_mounted(at) {
            return Err(Error::Argument(format!("path {at:?} is already mounted")));
        }
        self.entries.push(Grant::Mount(mount));
        Ok(())
    }

    pub(crate) fn is_mounted(&self, at: &str) -> bool {
        self.mounts().any(|mounted| mounted == at)
    }

    /// One invocation's Grants, an override standing in for the Mount at
    /// its path.
    pub(crate) fn for_invocation(&self, overrides: &[Mount]) -> Vec<Grant> {
        self.entries
            .iter()
            .map(|grant| {
                let overridden = to_mount_path(grant)
                    .and_then(|at| overrides.iter().find(|mount| mount.at() == at));
                match overridden {
                    Some(mount) => Grant::Mount(mount.clone()),
                    None => grant.clone(),
                }
            })
            .collect()
    }

    fn mounts(&self) -> impl Iterator<Item = &str> {
        self.entries.iter().filter_map(to_mount_path)
    }
}
