//! A Grant: a host resource the Host App hands the runtime instead of
//! binding it by name. The Sandbox holds it and each Invocation receives it
//! afresh; the guest reaches it through its system interface.

use crate::mount::Mount;

/// One resource handed over for an Invocation. Each kind carries what it
/// is and decides its own access.
#[derive(Clone)]
#[non_exhaustive]
pub enum Grant {
    Mount(Mount),
}

/// The Host App's decision for one request a Grant's access already allows,
/// so it can only refuse more, as an LSM hook can. Each kind of Grant brings
/// its own request, as Linux's LSM has a hook per kind of object.
pub trait Policy<R: ?Sized>: Send + Sync {
    /// Whether the guest may go on with `request`.
    fn allows(&self, request: &R) -> bool;
}

impl<R: ?Sized, F: Fn(&R) -> bool + Send + Sync> Policy<R> for F {
    fn allows(&self, request: &R) -> bool {
        self(request)
    }
}

/// The kinds of Grant a runtime may support.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum GrantKind {
    Mount,
}
