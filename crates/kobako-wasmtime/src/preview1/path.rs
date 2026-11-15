//! The 10 `path_*` calls. Each hands the call to the runtime's VFS, which
//! resolves the path, asks the Mount, and asks the Filesystem. What a Linux
//! syscall entry checks before the VFS stays here, such as the order of an
//! open's errors.

use kobako_runtime::filesystem::Iattr;
use kobako_runtime::vfs::{self, OpenHow, Start};
use wiggle::GuestPtr;

use super::descriptors::{Descriptor, FileHandle};
use super::errno::{Answer, Errno};
use super::fd::to_filestat;
use super::memory::GuestMemory;
use super::types;
use crate::preview2::filesystem::{self, retry_eagain};
use crate::wasi::Wasi;

/// The arguments `path_open` takes besides where it starts and the path, as
/// Linux's `openat2` takes a `struct open_how`; they become the `OpenHow` the
/// VFS opens with.
pub(crate) struct PathOpen {
    pub(crate) dirflags: types::Lookupflags,
    pub(crate) oflags: types::Oflags,
    pub(crate) rights_base: types::Rights,
    pub(crate) fdflags: types::Fdflags,
}

/// Where `fd` stands, for a path to be walked from, as the filesystem host
/// starts every walk from a descriptor.
fn start_of(wasi: &Wasi, fd: u32) -> impl vfs::Start + '_ {
    filesystem::start_at(wasi.walk_guard(), move || {
        wasi.file(fd).ok_or(kobako_runtime::Errno::BadDescriptor)
    })
}

pub(crate) fn create_directory(
    wasi: &Wasi,
    mem: &GuestMemory<'_, '_>,
    fd: u32,
    path: GuestPtr<str>,
) -> Answer {
    let path = mem.read_path(path)?;
    Ok(retry_eagain(|| {
        vfs::filename_mkdirat(wasi.cred(), &start_of(wasi, fd), &path)
    })?)
}

pub(crate) fn filestat_get(
    wasi: &Wasi,
    mem: &GuestMemory<'_, '_>,
    fd: u32,
    flags: types::Lookupflags,
    path: GuestPtr<str>,
) -> Answer<types::Filestat> {
    let path = mem.read_path(path)?;
    let follow = flags.contains(types::Lookupflags::SYMLINK_FOLLOW);
    let stat = retry_eagain(|| vfs::vfs_fstatat(wasi.cred(), &start_of(wasi, fd), &path, follow))?;
    Ok(to_filestat(wasi.cred(), stat))
}

pub(crate) fn filestat_set_times(
    wasi: &Wasi,
    mem: &GuestMemory<'_, '_>,
    fd: u32,
    flags: types::Lookupflags,
    path: GuestPtr<str>,
    times: &Iattr,
) -> Answer {
    let path = mem.read_path(path)?;
    let follow = flags.contains(types::Lookupflags::SYMLINK_FOLLOW);
    Ok(retry_eagain(|| {
        vfs::do_utimes_path(wasi.cred(), &start_of(wasi, fd), &path, follow, times)
    })?)
}

pub(crate) fn link(
    wasi: &Wasi,
    mem: &GuestMemory<'_, '_>,
    old: (u32, GuestPtr<str>),
    new: (u32, GuestPtr<str>),
    flags: types::Lookupflags,
) -> Answer {
    let (old_path, new_path) = (mem.read_path(old.1)?, mem.read_path(new.1)?);
    let follow = flags.contains(types::Lookupflags::SYMLINK_FOLLOW);
    Ok(retry_eagain(|| {
        let (from, to) = (start_of(wasi, old.0), start_of(wasi, new.0));
        vfs::filename_linkat(wasi.cred(), &from, &old_path, &to, &new_path, follow)
    })?)
}

pub(crate) fn open(
    wasi: &mut Wasi,
    mem: &GuestMemory<'_, '_>,
    fd: u32,
    path: GuestPtr<str>,
    call: &PathOpen,
) -> Answer<types::Fd> {
    let oflags = call.oflags;
    let mut how = OpenHow::default();
    // wasi-libc asks `fd_read` and `fd_readdir` for `O_RDONLY` and neither for
    // `O_SEARCH`, which glibc spells `O_PATH`; wasi-testsuite lists a
    // directory opened with `fd_readdir` alone, and makes a file asking no
    // right at all, which only an `O_RDONLY` open does.
    how.create = oflags.contains(types::Oflags::CREAT);
    how.exclusive = oflags.contains(types::Oflags::EXCL);
    how.truncate = oflags.contains(types::Oflags::TRUNC);
    how.write = call.rights_base.contains(types::Rights::FD_WRITE);
    how.read = call
        .rights_base
        .intersects(types::Rights::FD_READ | types::Rights::FD_READDIR)
        || (!how.write && (how.create || how.truncate));
    how.directory = oflags.contains(types::Oflags::DIRECTORY);
    how.follow = call.dirflags.contains(types::Lookupflags::SYMLINK_FOLLOW);
    how.append = call.fdflags.contains(types::Fdflags::APPEND);
    how.nonblock = call.fdflags.contains(types::Fdflags::NONBLOCK);
    how.dsync = call.fdflags.contains(types::Fdflags::DSYNC);
    how.sync = call
        .fdflags
        .intersects(types::Fdflags::RSYNC | types::Fdflags::SYNC);
    // Linux's `do_sys_openat2` builds the flags, then reserves a descriptor
    // and `path_openat` allocates the file before the walk answers for the
    // name, so a full table or a full memory answers whatever the path names.
    let op = vfs::build_open_flags(&how)?;
    let room = wasi.table().reserve_file::<FileHandle>()?;
    let new = wasi.descriptors().lowest_unused().ok_or(Errno::Mfile)?;
    let path = mem.read_path(path)?;
    let file = retry_eagain(|| vfs::do_file_open(wasi.cred(), &start_of(wasi, fd), &path, &op))?;
    let handle = wasi.table_mut().install(room, file);
    wasi.descriptors_mut()
        .place(new, Descriptor::for_file(handle));
    Ok(new.into())
}

pub(crate) fn readlink(
    wasi: &Wasi,
    mem: &mut GuestMemory<'_, '_>,
    fd: u32,
    path: GuestPtr<str>,
    buf: GuestPtr<u8>,
    buf_len: types::Size,
) -> Answer<types::Size> {
    if buf_len == 0 {
        return Err(Errno::Inval.into());
    }
    let path = mem.read_path(path)?;
    let target = retry_eagain(|| vfs::do_readlinkat(wasi.cred(), &start_of(wasi, fd), &path))?;
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
    wasi: &Wasi,
    mem: &GuestMemory<'_, '_>,
    fd: u32,
    path: GuestPtr<str>,
) -> Answer {
    let path = mem.read_path(path)?;
    Ok(retry_eagain(|| {
        vfs::filename_rmdir(wasi.cred(), &start_of(wasi, fd), &path)
    })?)
}

pub(crate) fn rename(
    wasi: &Wasi,
    mem: &GuestMemory<'_, '_>,
    old: (u32, GuestPtr<str>),
    new: (u32, GuestPtr<str>),
) -> Answer {
    let (from_path, to_path) = (mem.read_path(old.1)?, mem.read_path(new.1)?);
    Ok(retry_eagain(|| {
        let (from, to) = (start_of(wasi, old.0), start_of(wasi, new.0));
        vfs::filename_renameat2(wasi.cred(), &from, &from_path, &to, &to_path)
    })?)
}

pub(crate) fn symlink(
    wasi: &Wasi,
    mem: &GuestMemory<'_, '_>,
    old_path: GuestPtr<str>,
    fd: u32,
    new_path: GuestPtr<str>,
) -> Answer {
    let (target, path) = (mem.read_path(old_path)?, mem.read_path(new_path)?);
    // wasi-testsuite's symlink_create has a guest make no link to an absolute
    // path, where Linux would make one; a guest walks none of them anyway. It
    // is refused once the start is found, where the link would be made.
    let start = || -> Result<vfs::Path, kobako_runtime::Errno> {
        let start = start_of(wasi, fd).path_init()?;
        match target.starts_with('/') {
            true => Err(kobako_runtime::Errno::NotPermitted),
            false => Ok(start),
        }
    };
    Ok(retry_eagain(|| {
        vfs::filename_symlinkat(wasi.cred(), &target, &start, &path)
    })?)
}

pub(crate) fn unlink_file(
    wasi: &Wasi,
    mem: &GuestMemory<'_, '_>,
    fd: u32,
    path: GuestPtr<str>,
) -> Answer {
    let path = mem.read_path(path)?;
    Ok(retry_eagain(|| {
        vfs::filename_unlinkat(wasi.cred(), &start_of(wasi, fd), &path)
    })?)
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
