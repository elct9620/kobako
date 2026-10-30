//! The 10 `path_*` calls. Each hands the call to the runtime's VFS, which
//! resolves the path, asks the Mount, and asks the Filesystem. Links are no
//! Filesystem's to make, so those calls are unsupported.

use kobako_runtime::vfs::{self, OpenHow};
use wiggle::GuestPtr;

use super::errno::{Answer, Errno};
use super::fd::{check_fstflags, filestat};
use super::memory::GuestMemory;
use super::types;
use crate::descriptors::{Descriptor, Descriptors, OpenDirectory};

/// The flags `path_open` takes besides the path itself.
pub(crate) struct OpenFlags {
    pub(crate) dirflags: types::Lookupflags,
    pub(crate) oflags: types::Oflags,
    pub(crate) rights_base: types::Rights,
    pub(crate) fdflags: types::Fdflags,
}

/// The directory `fd` stands for; any other descriptor is `BADF`.
fn directory(table: &Descriptors, fd: u32) -> Answer<&vfs::Path> {
    match table.get(fd).ok_or(Errno::Badf)? {
        Descriptor::Directory(dir) => Ok(dir.file.path()),
        Descriptor::File(_) => Err(Errno::Badf.into()),
    }
}

pub(crate) fn create_directory(
    table: &Descriptors,
    mem: &GuestMemory<'_, '_>,
    fd: u32,
    path: GuestPtr<str>,
) -> Answer {
    let dir = directory(table, fd)?;
    Ok(vfs::filename_mkdirat(table.cred(), dir, &mem.path(path)?)?)
}

pub(crate) fn filestat_get(
    table: &Descriptors,
    mem: &GuestMemory<'_, '_>,
    fd: u32,
    flags: types::Lookupflags,
    path: GuestPtr<str>,
) -> Answer<types::Filestat> {
    let dir = directory(table, fd)?;
    let follow = flags.contains(types::Lookupflags::SYMLINK_FOLLOW);
    let metadata = vfs::vfs_fstatat(table.cred(), dir, &mem.path(path)?, follow)?;
    Ok(filestat(table, metadata))
}

pub(crate) fn filestat_set_times(
    table: &Descriptors,
    mem: &GuestMemory<'_, '_>,
    fd: u32,
    path: GuestPtr<str>,
    fst_flags: types::Fstflags,
) -> Answer {
    check_fstflags(fst_flags)?;
    let dir = directory(table, fd)?;
    Ok(vfs::do_utimes(table.cred(), dir, &mem.path(path)?)?)
}

/// A Filesystem holds no links, and answering as if one were made would
/// leave the guest believing it was.
pub(crate) fn link(
    table: &Descriptors,
    mem: &GuestMemory<'_, '_>,
    old: (u32, GuestPtr<str>),
    new: (u32, GuestPtr<str>),
) -> Answer {
    directory(table, old.0)?;
    directory(table, new.0)?;
    mem.path(old.1)?;
    mem.path(new.1)?;
    Err(Errno::Notsup.into())
}

pub(crate) fn open(
    table: &mut Descriptors,
    mem: &GuestMemory<'_, '_>,
    fd: u32,
    path: GuestPtr<str>,
    flags: &OpenFlags,
) -> Answer<types::Fd> {
    let path = mem.path(path)?;
    let dir = match table.get(fd).ok_or(Errno::Badf)? {
        Descriptor::Directory(dir) => dir.file.path(),
        Descriptor::File(_) => return Err(Errno::Notdir.into()),
    };
    // A Filesystem makes no write durable on its own schedule, and every
    // call waits for its answer, so an open asking otherwise is
    // unsupported rather than quietly granted.
    if flags.fdflags.intersects(
        types::Fdflags::DSYNC
            | types::Fdflags::RSYNC
            | types::Fdflags::SYNC
            | types::Fdflags::NONBLOCK,
    ) {
        return Err(Errno::Notsup.into());
    }
    let oflags = flags.oflags;
    if oflags.contains(types::Oflags::DIRECTORY)
        && oflags.intersects(types::Oflags::CREAT | types::Oflags::EXCL | types::Oflags::TRUNC)
    {
        return Err(Errno::Inval.into());
    }
    let mut how = OpenHow::default();
    how.read = flags.rights_base.contains(types::Rights::FD_READ);
    how.write = flags.rights_base.contains(types::Rights::FD_WRITE);
    how.create = oflags.contains(types::Oflags::CREAT);
    how.exclusive = oflags.contains(types::Oflags::EXCL);
    how.truncate = oflags.contains(types::Oflags::TRUNC);
    how.directory = oflags.contains(types::Oflags::DIRECTORY);
    how.follow = flags.dirflags.contains(types::Lookupflags::SYMLINK_FOLLOW);
    how.append = flags.fdflags.contains(types::Fdflags::APPEND);
    let file = vfs::do_file_open(table.cred(), dir, &path, &how, !table.is_full())?;
    let descriptor = match file.is_directory() {
        true => Descriptor::Directory(OpenDirectory {
            file,
            preopen: None,
        }),
        false => Descriptor::File(file),
    };
    Ok(table.push(descriptor).ok_or(Errno::Mfile)?.into())
}

pub(crate) fn readlink(
    table: &Descriptors,
    mem: &GuestMemory<'_, '_>,
    fd: u32,
    path: GuestPtr<str>,
) -> Answer<types::Size> {
    let dir = directory(table, fd)?;
    vfs::do_readlinkat(table.cred(), dir, &mem.path(path)?)?;
    Err(Errno::Notsup.into())
}

pub(crate) fn remove_directory(
    table: &Descriptors,
    mem: &GuestMemory<'_, '_>,
    fd: u32,
    path: GuestPtr<str>,
) -> Answer {
    let dir = directory(table, fd)?;
    Ok(vfs::filename_rmdir(table.cred(), dir, &mem.path(path)?)?)
}

pub(crate) fn rename(
    table: &Descriptors,
    mem: &GuestMemory<'_, '_>,
    old: (u32, GuestPtr<str>),
    new: (u32, GuestPtr<str>),
) -> Answer {
    let from = directory(table, old.0)?;
    let to = directory(table, new.0)?;
    let (from_path, to_path) = (mem.path(old.1)?, mem.path(new.1)?);
    Ok(vfs::filename_renameat2(
        table.cred(),
        from,
        &from_path,
        to,
        &to_path,
    )?)
}

/// A Filesystem holds no links to make.
pub(crate) fn symlink(
    table: &Descriptors,
    mem: &GuestMemory<'_, '_>,
    old_path: GuestPtr<str>,
    fd: u32,
    new_path: GuestPtr<str>,
) -> Answer {
    directory(table, fd)?;
    mem.path(old_path)?;
    mem.path(new_path)?;
    Err(Errno::Notsup.into())
}

pub(crate) fn unlink_file(
    table: &Descriptors,
    mem: &GuestMemory<'_, '_>,
    fd: u32,
    path: GuestPtr<str>,
) -> Answer {
    let dir = directory(table, fd)?;
    Ok(vfs::filename_unlinkat(table.cred(), dir, &mem.path(path)?)?)
}
