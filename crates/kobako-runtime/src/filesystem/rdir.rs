//! What a Filesystem reading a whole directory at once holds while a listing
//! goes on, as 9p's `fid->rdir` holds what its server sent, so a caller reading in
//! small steps costs one listing, not one per step.

use std::sync::Arc;

use crate::vfs::{Cred, Dentry, DirContext, DirEntry, FileType};
use crate::Errno;

/// The entries a listing began with, `.` and `..` first, on the open directory
/// until it reaches its end, counted against who holds them.
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
    fn account(cred: &Arc<Cred>, entries: &[DirEntry]) -> Result<Self, Errno> {
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
    /// Hand `ctx` the directory from its position. A listing from the start
    /// reads the directory through `read`, then stats it and what stands above
    /// it for `.` and `..`, as 9p and FUSE take those from their server and
    /// `ovl_cache_update` asks `vfs_getattr`; a failure of either answers before
    /// any entry. A listing going on reads what that listing began with. Those
    /// entries count against the caller from the moment they are read until the
    /// listing ends, as `ovl_cache_get` and `v9fs_alloc_rdir_buf` allocate before
    /// the first `dir_emit`: past what a security module allows, the listing
    /// answers out of memory before it hands any entry.
    pub fn iterate(
        &mut self,
        dentry: &Dentry,
        ctx: &mut DirContext<'_>,
        read: impl FnOnce() -> Result<Vec<DirEntry>, Errno>,
    ) -> Result<(), Errno> {
        let listing = match self.0.take().filter(|_| ctx.pos() != 0) {
            Some(held) => held,
            None => {
                let read = read()?;
                let mut entries = dots(dentry)?;
                entries.extend(read);
                entries.shrink_to_fit();
                let _charge = Charge::account(ctx.cred(), &entries)?;
                Listing { entries, _charge }
            }
        };
        if !emit_from(ctx, &listing.entries) {
            self.0 = Some(listing);
        }
        Ok(())
    }
}

/// `.` and `..` as a stat of `dentry` and of what stands above it reports them,
/// `..` naming `dentry` itself at the root.
fn dots(dentry: &Dentry) -> Result<Vec<DirEntry>, Errno> {
    let dot = dentry.getattr()?;
    let dotdot = dentry.parent().getattr()?;
    Ok(vec![
        DirEntry::new(".", FileType::Dir).with_identity(dot.dev, dot.ino),
        DirEntry::new("..", FileType::Dir).with_identity(dotdot.dev, dotdot.ino),
    ])
}

/// Hand `ctx` `entries` from where it stands, each at its index, answering
/// whether it took them all.
fn emit_from(ctx: &mut DirContext<'_>, entries: &[DirEntry]) -> bool {
    let skip = usize::try_from(ctx.pos()).unwrap_or(usize::MAX);
    entries.iter().skip(skip).all(|entry| {
        let took = ctx.dir_emit(entry);
        if took {
            ctx.set_pos(ctx.pos() + 1);
        }
        took
    })
}
