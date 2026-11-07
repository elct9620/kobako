//! The Grants a Sandbox hands each invocation. They seal with the rest of
//! the Catalog, and each invocation receives them afresh.

use kobako_runtime::grant::Grant;
use kobako_runtime::mount::Mount;

use crate::error::Error;

/// Whether `grant` and `other` are the same kind of Grant at one key, so
/// one stands in for the other: a Mount at the same path.
pub(crate) fn shares_key(grant: &Grant, other: &Grant) -> bool {
    match (grant, other) {
        (Grant::Mount(grant), Grant::Mount(other)) => grant.at() == other.at(),
        _ => false,
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
        let at = mount.at().to_string();
        let grant = Grant::Mount(mount);
        if self.holds(&grant) {
            return Err(Error::Argument(format!("path {at:?} is already mounted")));
        }
        self.entries.push(grant);
        Ok(())
    }

    /// Whether a Grant of `grant`'s kind stands at its key.
    pub(crate) fn holds(&self, grant: &Grant) -> bool {
        self.entries.iter().any(|entry| shares_key(entry, grant))
    }

    /// One invocation's Grants, each override standing in for the Grant of
    /// its kind at its key.
    pub(crate) fn for_invocation(&self, overrides: &[Grant]) -> Vec<Grant> {
        self.entries
            .iter()
            .map(|grant| {
                overrides
                    .iter()
                    .find(|other| shares_key(grant, other))
                    .unwrap_or(grant)
                    .clone()
            })
            .collect()
    }
}
