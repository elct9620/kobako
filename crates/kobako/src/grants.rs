//! The Grants a Sandbox hands each invocation. They seal with the rest of
//! the Catalog, and each invocation receives them afresh.

use kobako_runtime::grant::Grant;
use kobako_runtime::mount::Mount;

use crate::error::Error;

/// An absolute guest path has no empty, `.`, or `..` component; the root
/// is `/` alone.
fn is_guest_path(at: &str) -> bool {
    at == "/"
        || at.strip_prefix('/').is_some_and(|rest| {
            rest.split('/')
                .all(|part| !part.is_empty() && part != "." && part != "..")
        })
}

fn nested(path: &str, other: &str) -> bool {
    path == "/"
        || other == "/"
        || path.starts_with(&format!("{other}/"))
        || other.starts_with(&format!("{path}/"))
}

fn mounted_at(grant: &Grant) -> Option<&str> {
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
    /// A path already mounted, or one beneath or above another Mount, is
    /// refused, as a bind is.
    pub(crate) fn mount(&mut self, mount: Mount) -> Result<(), Error> {
        let at = mount.at();
        if !is_guest_path(at) {
            return Err(Error::Argument(format!(
                "mount path must be an absolute guest path, got {at:?}"
            )));
        }
        if self.is_mounted(at) {
            return Err(Error::Argument(format!("path {at:?} is already mounted")));
        }
        if let Some(other) = self.mounts().find(|other| nested(at, other)) {
            return Err(Error::Argument(format!(
                "path {at:?} nests with the mounted path {other:?}"
            )));
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
                let overridden = mounted_at(grant)
                    .and_then(|at| overrides.iter().find(|mount| mount.at() == at));
                match overridden {
                    Some(mount) => Grant::Mount(mount.clone()),
                    None => grant.clone(),
                }
            })
            .collect()
    }

    fn mounts(&self) -> impl Iterator<Item = &str> {
        self.entries.iter().filter_map(mounted_at)
    }
}
