//! A directory's extended attributes, as tmpfs keeps the `trusted.*`
//! ones overlayfs marks a directory opaque with.

use super::super::FilesystemError;
use super::Tmpfs;

impl Tmpfs {
    /// The value of the attribute `name` on this directory, as `getxattr`
    /// reads it, or `None` where it is not set.
    pub(crate) fn getxattr(&self, name: &str) -> Option<Vec<u8>> {
        self.dir.state().xattrs.get(name).cloned()
    }

    /// Set the attribute `name` on this directory, as `setxattr` does.
    pub(crate) fn setxattr(&self, name: &str, value: &[u8]) -> Result<(), FilesystemError> {
        self.dir
            .state()
            .xattrs
            .insert(name.to_string(), value.to_vec());
        Ok(())
    }

    /// Take the attribute `name` off this directory, as `removexattr`
    /// does.
    pub(crate) fn removexattr(&self, name: &str) {
        self.dir.state().xattrs.remove(name);
    }
}
