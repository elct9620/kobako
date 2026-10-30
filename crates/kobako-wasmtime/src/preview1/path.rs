//! The 10 `path_*` calls. Each hands the call to the runtime's VFS, which
//! resolves the path, asks the Mount, and asks the Filesystem.

use kobako_runtime::filesystem::Iattr;
use kobako_runtime::vfs::{self, OpenHow};
use wiggle::GuestPtr;

use super::errno::{Answer, Errno};
use super::fd::filestat;
use super::memory::GuestMemory;
use super::types;
use crate::descriptors::{Descriptor, Descriptors};

/// The flags `path_open` takes besides the path itself.
pub(crate) struct OpenFlags {
    pub(crate) dirflags: types::Lookupflags,
    pub(crate) oflags: types::Oflags,
    pub(crate) rights_base: types::Rights,
    pub(crate) fdflags: types::Fdflags,
}

/// The directory `fd` stands for, a path being walked from; anything else is
/// `NOTDIR`, as Linux refuses a walk starting there.
fn directory(table: &Descriptors, fd: u32) -> Answer<&vfs::Path> {
    let file = &table.get(fd).ok_or(Errno::Badf)?.file;
    match file.is_directory() {
        true => Ok(file.path()),
        false => Err(Errno::Notdir.into()),
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
    flags: types::Lookupflags,
    path: GuestPtr<str>,
    times: Option<Iattr>,
) -> Answer {
    let dir = directory(table, fd)?;
    let path = mem.path(path)?;
    let follow = flags.contains(types::Lookupflags::SYMLINK_FOLLOW);
    match times {
        Some(times) => Ok(vfs::do_utimes(table.cred(), dir, &path, follow, &times)?),
        None => Ok(()),
    }
}

pub(crate) fn link(
    table: &Descriptors,
    mem: &GuestMemory<'_, '_>,
    old: (u32, GuestPtr<str>),
    new: (u32, GuestPtr<str>),
    flags: types::Lookupflags,
) -> Answer {
    let from = directory(table, old.0)?;
    let to = directory(table, new.0)?;
    let (old_path, new_path) = (mem.path(old.1)?, mem.path(new.1)?);
    let follow = flags.contains(types::Lookupflags::SYMLINK_FOLLOW);
    Ok(vfs::filename_linkat(
        table.cred(),
        from,
        &old_path,
        to,
        &new_path,
        follow,
    )?)
}

pub(crate) fn open(
    table: &mut Descriptors,
    mem: &GuestMemory<'_, '_>,
    fd: u32,
    path: GuestPtr<str>,
    flags: &OpenFlags,
) -> Answer<types::Fd> {
    let path = mem.path(path)?;
    let dir = directory(table, fd)?;
    let oflags = flags.oflags;
    let mut how = OpenHow::default();
    how.read = flags.rights_base.contains(types::Rights::FD_READ);
    how.write = flags.rights_base.contains(types::Rights::FD_WRITE);
    how.create = oflags.contains(types::Oflags::CREAT);
    how.exclusive = oflags.contains(types::Oflags::EXCL);
    how.truncate = oflags.contains(types::Oflags::TRUNC);
    how.directory = oflags.contains(types::Oflags::DIRECTORY);
    how.follow = flags.dirflags.contains(types::Lookupflags::SYMLINK_FOLLOW);
    how.append = flags.fdflags.contains(types::Fdflags::APPEND);
    how.nonblock = flags.fdflags.contains(types::Fdflags::NONBLOCK);
    how.sync = flags
        .fdflags
        .intersects(types::Fdflags::DSYNC | types::Fdflags::RSYNC | types::Fdflags::SYNC);
    let file = vfs::do_file_open(table.cred(), dir, &path, &how, !table.is_full())?;
    Ok(table.push(Descriptor::of(file)).ok_or(Errno::Mfile)?.into())
}

pub(crate) fn readlink(
    table: &Descriptors,
    mem: &mut GuestMemory<'_, '_>,
    fd: u32,
    path: GuestPtr<str>,
    buf: GuestPtr<u8>,
    buf_len: types::Size,
) -> Answer<types::Size> {
    let dir = directory(table, fd)?;
    let target = vfs::do_readlinkat(table.cred(), dir, &mem.path(path)?)?;
    write_link(mem, &target, buf, buf_len)
}

/// As much of where a link points as the buffer holds, as `readlink` cuts it
/// off there without saying so.
fn write_link(
    mem: &mut GuestMemory<'_, '_>,
    target: &str,
    buf: GuestPtr<u8>,
    buf_len: types::Size,
) -> Answer<types::Size> {
    let take = target.len().min(buf_len as usize);
    mem.write_bytes(buf, &target.as_bytes()[..take])?;
    Ok(take as u32)
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

pub(crate) fn symlink(
    table: &Descriptors,
    mem: &GuestMemory<'_, '_>,
    old_path: GuestPtr<str>,
    fd: u32,
    new_path: GuestPtr<str>,
) -> Answer {
    let dir = directory(table, fd)?;
    let (target, path) = (mem.path(old_path)?, mem.path(new_path)?);
    Ok(vfs::filename_symlinkat(table.cred(), &target, dir, &path)?)
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

#[cfg(test)]
mod tests {
    use super::*;

    // Every Invocation holds to the hook refusing a readlink, so no guest call
    // reaches this; only the encoding itself can witness it.
    #[test]
    fn a_link_is_cut_off_where_the_buffer_ends() {
        let mut bytes = [0u8; 8];
        let answered = {
            let mut memory = wiggle::GuestMemory::Unshared(&mut bytes);
            let mut mem = GuestMemory::new(&mut memory, 64);
            write_link(&mut mem, "target", GuestPtr::new(2), 3)
                .unwrap_or_else(|_| panic!("the link must write"))
        };
        assert_eq!(
            (answered, &bytes),
            (3, b"\0\0tar\0\0\0"),
            "where a link points through path_readlink must fill the buffer and answer what fit"
        );
    }
}
