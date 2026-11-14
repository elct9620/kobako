//! A network entry's connect policy as a security module, as Landlock's
//! `net.c` answers the network rights of its domain.

use std::sync::Arc;

use super::SecurityOperations;
use crate::network::Connection;
use crate::policy::Policy;
use crate::Errno;

/// An entry's connect policy, judging what the guest named, as one more LSM
/// answers `security_socket_connect`.
pub(super) struct EntryPolicy(pub(super) Arc<dyn for<'a> Policy<Connection<'a>>>);

impl SecurityOperations for EntryPolicy {
    fn socket_connect(&self, connection: &Connection<'_>) -> Result<(), Errno> {
        match self.0.allows(connection) {
            true => Ok(()),
            false => Err(Errno::PermissionDenied),
        }
    }
}
