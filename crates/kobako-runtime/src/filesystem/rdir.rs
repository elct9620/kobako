//! What a Filesystem reading a whole directory at once holds while a listing
//! goes on, as 9p's `fid->rdir` holds what its server sent, so a caller reading in
//! small steps costs one listing, not one per step.

use std::sync::Arc;

use crate::vfs::{dir_emit_entries, Cred, Dentry, DirContext, DirEntry, FilesystemError};

/// The entries a listing began with, on the open directory until it reaches
/// its end, counted against who holds them.
#[derive(Default)]
pub struct Rdir(Option<Listing>);

struct Listing {
    entries: Vec<DirEntry>,
    /// Gives the entries' bytes back once the listing goes.
    _charge: Charge,
}

/// Bytes a listing counts for as long as it holds them, as shmem pairs
/// `shmem_acct_size` with `shmem_unacct_size`.
struct Charge {
    cred: Arc<Cred>,
    bytes: usize,
}

impl Charge {
    fn of(cred: &Arc<Cred>, entries: &[DirEntry]) -> Result<Self, FilesystemError> {
        let names: usize = entries.iter().map(|entry| entry.name.capacity()).sum();
        let bytes = size_of_val(entries) + names;
        cred.security_vm_enough_memory(bytes)?;
        Ok(Self {
            cred: cred.clone(),
            bytes,
        })
    }
}

impl Drop for Charge {
    fn drop(&mut self) {
        self.cred.vm_unacct_memory(self.bytes);
    }
}

impl Rdir {
    /// Hand `ctx` the directory from its position, `.` and `..` first. A listing
    /// from the start reads the directory through `read`, and one going on reads
    /// what that listing began with. Those entries count against the caller from
    /// the moment they are read until the listing ends, as `ovl_cache_get` and
    /// `v9fs_alloc_rdir_buf` allocate before the first `dir_emit`: past what a
    /// security module allows, the listing answers out of memory before it hands
    /// any entry.
    pub fn iterate(
        &mut self,
        dentry: &Dentry,
        ctx: &mut DirContext<'_>,
        read: impl FnOnce() -> Result<Vec<DirEntry>, FilesystemError>,
    ) -> Result<(), FilesystemError> {
        let listing = match self.0.take().filter(|_| ctx.pos() != 0) {
            Some(held) => held,
            None => {
                let mut entries = read()?;
                entries.shrink_to_fit();
                let _charge = Charge::of(ctx.cred(), &entries)?;
                Listing { entries, _charge }
            }
        };
        if !dir_emit_entries(dentry, ctx, &listing.entries) {
            self.0 = Some(listing);
        }
        Ok(())
    }
}
