//! Files for what the runtime keeps that is no file of its own, as Linux's
//! anon_inodefs makes one for an eventfd or a timerfd: every such file stands
//! on one inode no path reaches, and only its operations tell them apart, so
//! any ABI a Wasm Driver encodes reads, writes, and polls them as it does any
//! other open file.

use std::sync::{Arc, LazyLock};

use crate::cred::Cred;
use crate::vfs::{
    alloc_file_pseudo, Dentry, File, FileOperations, FileSystemType, FileType, Inode,
    InodeOperations, Kstat, OpenHow, SuperBlock, VfsMount,
};
use crate::Errno;

/// The one Filesystem every anonymous file stands on, as Linux's
/// `anon_inode_mnt`.
static ANON_INODE_MNT: LazyLock<Arc<VfsMount>> = LazyLock::new(|| {
    let sb = SuperBlock::new(Arc::new(AnonInodeFs))
        .expect("anon_inodefs fills in nothing that can fail");
    VfsMount::kern_mount(sb)
});

/// The one inode every anonymous file shares, as Linux's `anon_inode_inode`.
static ANON_INODE_INODE: LazyLock<Arc<Inode>> =
    LazyLock::new(|| Inode::new(&Kstat::new(FileType::Unknown, 0), Arc::new(AnonInodeFs)));

/// An anonymous inode reports no kind and no length, as `alloc_anon_inode`
/// sets no file type in its mode.
struct AnonInodeFs;

impl FileSystemType for AnonInodeFs {
    fn fill_super(self: Arc<Self>, sb: &mut SuperBlock) -> Result<(), Errno> {
        sb.d_make_root(Inode::new(&Kstat::new(FileType::Dir, 0), self));
        Ok(())
    }
}

impl InodeOperations for AnonInodeFs {
    fn getattr(&self, _dentry: &Dentry) -> Result<Kstat, Errno> {
        Ok(Kstat::new(FileType::Unknown, 0))
    }
}

/// A file `ops` answers for, opened as `how` asks and as `cred`, on the one
/// anonymous inode, as Linux's `anon_inode_getfile` makes one.
pub(crate) fn anon_inode_getfile(
    ops: Box<dyn FileOperations>,
    how: &OpenHow,
    cred: &Arc<Cred>,
) -> File {
    alloc_file_pseudo(ANON_INODE_INODE.clone(), &ANON_INODE_MNT, how, ops, cred)
}
