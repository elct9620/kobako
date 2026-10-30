//! The 10 `path_*` calls. Each reads its arguments from guest memory and
//! hands the call to the runtime's VFS, which resolves the path, asks the
//! Mount, and asks the Filesystem. Links are no Filesystem's to make, so
//! those calls are unsupported.

use std::sync::Arc;

use kobako_runtime::vfs::{self, FileDescription, OpenHow};

use super::args::{
    self, FDFLAGS_APPEND, FDFLAGS_NONBLOCK, FDFLAGS_SYNCHRONIZED, RIGHTS_FD_READ, RIGHTS_FD_WRITE,
};
use super::errno::{Answer, Errno};
use super::fd::write_metadata;
use super::memory::GuestMemory;
use crate::descriptors::{Descriptor, Descriptors, OpenDirectory, OpenFile};

const LOOKUPFLAGS_SYMLINK_FOLLOW: u32 = 1 << 0;
const OFLAGS_CREAT: u32 = 1 << 0;
const OFLAGS_DIRECTORY: u32 = 1 << 1;
const OFLAGS_EXCL: u32 = 1 << 2;
const OFLAGS_TRUNC: u32 = 1 << 3;

/// The flags `path_open` takes besides the path itself.
pub(crate) struct OpenFlags {
    pub(crate) dirflags: u32,
    pub(crate) oflags: u32,
    pub(crate) rights_base: u64,
    pub(crate) rights_inheriting: u64,
    pub(crate) fdflags: u32,
}

/// The directory `fd` stands for; any other descriptor is `BADF`.
fn directory(table: &Descriptors, fd: u32) -> Answer<&vfs::Path> {
    match table.get(fd).ok_or(Errno::Badf)? {
        Descriptor::Directory(dir) => Ok(&dir.path),
        Descriptor::Stdin(_) | Descriptor::Channel(_) | Descriptor::File(_) => {
            Err(Errno::Badf.into())
        }
    }
}

pub(crate) fn create_directory(
    table: &Descriptors,
    mem: &GuestMemory,
    fd: u32,
    path: (u32, u32),
) -> Answer {
    let dir = directory(table, fd)?;
    Ok(vfs::filename_mkdirat(dir, &mem.path(path.0, path.1)?)?)
}

pub(crate) fn filestat_get(
    table: &Descriptors,
    mem: &mut GuestMemory,
    fd: u32,
    flags: u32,
    path: (u32, u32),
    out: u32,
) -> Answer {
    args::flags(flags, args::LOOKUPFLAGS)?;
    let dir = directory(table, fd)?;
    let path = mem.path(path.0, path.1)?;
    let metadata = if flags & LOOKUPFLAGS_SYMLINK_FOLLOW != 0 {
        vfs::vfs_fstatat(dir, &path, true)?
    } else {
        vfs::vfs_fstatat(dir, &path, false)?
    };
    write_metadata(mem, out, metadata)
}

pub(crate) fn filestat_set_times(
    table: &Descriptors,
    mem: &GuestMemory,
    fd: u32,
    flags: u32,
    path: (u32, u32),
    fst_flags: u32,
) -> Answer {
    args::flags(flags, args::LOOKUPFLAGS)?;
    args::fstflags(fst_flags)?;
    let dir = directory(table, fd)?;
    Ok(vfs::do_utimes(dir, &mem.path(path.0, path.1)?)?)
}

/// A Filesystem holds no links, and answering as if one were made would
/// leave the guest believing it was.
pub(crate) fn link(
    table: &Descriptors,
    mem: &GuestMemory,
    old_fd: u32,
    old_flags: u32,
    old_path: (u32, u32),
    new_fd: u32,
    new_path: (u32, u32),
) -> Answer {
    args::flags(old_flags, args::LOOKUPFLAGS)?;
    directory(table, old_fd)?;
    directory(table, new_fd)?;
    mem.path(old_path.0, old_path.1)?;
    mem.path(new_path.0, new_path.1)?;
    Err(Errno::Notsup.into())
}

pub(crate) fn open(
    table: &mut Descriptors,
    mem: &mut GuestMemory,
    fd: u32,
    path: (u32, u32),
    flags: &OpenFlags,
    out: u32,
) -> Answer {
    args::flags(flags.dirflags, args::LOOKUPFLAGS)?;
    args::flags(flags.oflags, args::OFLAGS)?;
    args::flags(flags.rights_base, args::RIGHTS)?;
    args::flags(flags.rights_inheriting, args::RIGHTS)?;
    args::flags(flags.fdflags, args::FDFLAGS)?;
    let path = mem.path(path.0, path.1)?;
    let dir = match table.get(fd).ok_or(Errno::Badf)? {
        Descriptor::Directory(dir) => &dir.path,
        Descriptor::File(_) => return Err(Errno::Notdir.into()),
        Descriptor::Stdin(_) | Descriptor::Channel(_) => return Err(Errno::Badf.into()),
    };
    let oflag = |bit| flags.oflags & bit != 0;
    // A Filesystem makes no write durable on its own schedule, and every
    // call waits for its answer, so an open asking otherwise is
    // unsupported rather than quietly granted.
    if flags.fdflags & (FDFLAGS_SYNCHRONIZED | FDFLAGS_NONBLOCK) != 0 {
        return Err(Errno::Notsup.into());
    }
    if oflag(OFLAGS_DIRECTORY) && oflag(OFLAGS_CREAT | OFLAGS_EXCL | OFLAGS_TRUNC) {
        return Err(Errno::Inval.into());
    }
    let mut how = OpenHow::default();
    how.read = flags.rights_base & RIGHTS_FD_READ != 0;
    how.write = flags.rights_base & RIGHTS_FD_WRITE != 0;
    how.create = oflag(OFLAGS_CREAT);
    how.exclusive = oflag(OFLAGS_EXCL);
    how.truncate = oflag(OFLAGS_TRUNC);
    how.directory = oflag(OFLAGS_DIRECTORY);
    how.follow = flags.dirflags & LOOKUPFLAGS_SYMLINK_FOLLOW != 0;
    let mount = Arc::clone(dir.mount());
    let descriptor = match vfs::do_file_open(dir, &path, &how, !table.is_full())? {
        FileDescription::Directory(path) => Descriptor::Directory(OpenDirectory {
            path,
            preopen: false,
            listing: None,
        }),
        FileDescription::File(file) => Descriptor::File(OpenFile {
            file,
            position: 0,
            append: flags.fdflags & FDFLAGS_APPEND != 0,
            readable: how.read,
            writable: how.write,
            mount,
        }),
    };
    let opened = table.push(descriptor).ok_or(Errno::Mfile)?;
    mem.write_u32(out, opened)
}

pub(crate) fn readlink(
    table: &Descriptors,
    mem: &GuestMemory,
    fd: u32,
    path: (u32, u32),
) -> Answer {
    let dir = directory(table, fd)?;
    Ok(vfs::do_readlinkat(dir, &mem.path(path.0, path.1)?)?)
}

pub(crate) fn remove_directory(
    table: &Descriptors,
    mem: &GuestMemory,
    fd: u32,
    path: (u32, u32),
) -> Answer {
    let dir = directory(table, fd)?;
    Ok(vfs::filename_rmdir(dir, &mem.path(path.0, path.1)?)?)
}

pub(crate) fn rename(
    table: &Descriptors,
    mem: &GuestMemory,
    old: (u32, (u32, u32)),
    new: (u32, (u32, u32)),
) -> Answer {
    let from = directory(table, old.0)?;
    let to = directory(table, new.0)?;
    let (from_path, to_path) = (mem.path(old.1 .0, old.1 .1)?, mem.path(new.1 .0, new.1 .1)?);
    Ok(vfs::filename_renameat2(from, &from_path, to, &to_path)?)
}

/// A Filesystem holds no links to make.
pub(crate) fn symlink(
    table: &Descriptors,
    mem: &GuestMemory,
    old_path: (u32, u32),
    fd: u32,
    new_path: (u32, u32),
) -> Answer {
    directory(table, fd)?;
    mem.path(old_path.0, old_path.1)?;
    mem.path(new_path.0, new_path.1)?;
    Err(Errno::Notsup.into())
}

pub(crate) fn unlink_file(
    table: &Descriptors,
    mem: &GuestMemory,
    fd: u32,
    path: (u32, u32),
) -> Answer {
    let dir = directory(table, fd)?;
    Ok(vfs::filename_unlinkat(dir, &mem.path(path.0, path.1)?)?)
}
