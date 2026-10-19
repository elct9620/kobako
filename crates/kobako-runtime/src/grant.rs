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

/// The kinds of Grant a runtime may support.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum GrantKind {
    Mount,
}
