//! A Policy: the Host App's decision over one request a Grant's guest makes.
//! Each kind of Grant names its own request, so this depends on none of them.

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
