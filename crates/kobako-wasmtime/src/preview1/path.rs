//! The 10 `path_*` calls. Each hands the call to the runtime's VFS, which
//! resolves the path, asks the Mount, and asks the Filesystem.

use kobako_runtime::filesystem::{FilesystemError, Iattr};
use kobako_runtime::vfs::{self, OpenHow, Start};
use wiggle::GuestPtr;

use super::errno::{Answer, Errno};
use super::fd::to_filestat;
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

/// How many times a call is made before a guest is answered EAGAIN, as cap-std
/// makes `openat2` four times: any move in the process answers a `..` walking
/// past it so, and nothing bounds how often one lands.
const MAX_ATTEMPTS: usize = 4;

/// What `call` answers once no move landed during its walk, made again at most
/// `MAX_ATTEMPTS` times in all, as cap-std retries `openat2` on EAGAIN.
pub(super) fn retry_eagain<T>(
    mut call: impl FnMut() -> Result<T, FilesystemError>,
) -> Result<T, FilesystemError> {
    let mut answer = call();
    for _ in 1..MAX_ATTEMPTS {
        if !matches!(answer, Err(FilesystemError::WouldBlock)) {
            break;
        }
        answer = call();
    }
    answer
}

/// Where `fd` stands, for a path to be walked from, once the guard answered for a
/// directory there, found when the VFS has taken the name, as Linux's
/// `path_init` takes the descriptor. What is no directory the walk itself
/// refuses, and a move the guard meets is one a walk meets, so `retry_eagain`
/// makes the call again.
fn start_of(table: &Descriptors, fd: u32) -> impl vfs::Start + '_ {
    move || {
        let file = &table.get(fd).ok_or(FilesystemError::BadDescriptor)?.file;
        if file.is_directory() {
            table.walk_guard().check(file.path())?;
        }
        Ok(file.path().clone())
    }
}

pub(crate) fn create_directory(
    table: &Descriptors,
    mem: &GuestMemory<'_, '_>,
    fd: u32,
    path: GuestPtr<str>,
) -> Answer {
    let path = mem.path(path)?;
    Ok(retry_eagain(|| {
        vfs::filename_mkdirat(table.cred(), &start_of(table, fd), &path)
    })?)
}

pub(crate) fn filestat_get(
    table: &Descriptors,
    mem: &GuestMemory<'_, '_>,
    fd: u32,
    flags: types::Lookupflags,
    path: GuestPtr<str>,
) -> Answer<types::Filestat> {
    let path = mem.path(path)?;
    let follow = flags.contains(types::Lookupflags::SYMLINK_FOLLOW);
    let stat =
        retry_eagain(|| vfs::vfs_fstatat(table.cred(), &start_of(table, fd), &path, follow))?;
    Ok(to_filestat(table, stat))
}

pub(crate) fn filestat_set_times(
    table: &Descriptors,
    mem: &GuestMemory<'_, '_>,
    fd: u32,
    flags: types::Lookupflags,
    path: GuestPtr<str>,
    times: Option<Iattr>,
) -> Answer {
    // Asking no time does nothing, so Linux's `utimensat` answers before it
    // takes the name or the descriptor.
    let Some(times) = times else {
        return Ok(());
    };
    let path = mem.path(path)?;
    let follow = flags.contains(types::Lookupflags::SYMLINK_FOLLOW);
    Ok(retry_eagain(|| {
        vfs::do_utimes(table.cred(), &start_of(table, fd), &path, follow, &times)
    })?)
}

pub(crate) fn link(
    table: &Descriptors,
    mem: &GuestMemory<'_, '_>,
    old: (u32, GuestPtr<str>),
    new: (u32, GuestPtr<str>),
    flags: types::Lookupflags,
) -> Answer {
    let (old_path, new_path) = (mem.path(old.1)?, mem.path(new.1)?);
    let follow = flags.contains(types::Lookupflags::SYMLINK_FOLLOW);
    Ok(retry_eagain(|| {
        let (from, to) = (start_of(table, old.0), start_of(table, new.0));
        vfs::filename_linkat(table.cred(), &from, &old_path, &to, &new_path, follow)
    })?)
}

pub(crate) fn open(
    table: &mut Descriptors,
    mem: &GuestMemory<'_, '_>,
    fd: u32,
    path: GuestPtr<str>,
    flags: &OpenFlags,
) -> Answer<types::Fd> {
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
    how.dsync = flags.fdflags.contains(types::Fdflags::DSYNC);
    how.sync = flags
        .fdflags
        .intersects(types::Fdflags::RSYNC | types::Fdflags::SYNC);
    // Linux's `do_sys_openat2` builds the flags, then reserves a descriptor
    // before `do_file_open` answers for the name, so a full table answers
    // whatever the path names.
    let op = vfs::build_open_flags(&how)?;
    let path = mem.path(path)?;
    let new = table.get_unused_fd().ok_or(Errno::Mfile)?;
    let file = retry_eagain(|| vfs::do_file_open(table.cred(), &start_of(table, fd), &path, &op))?;
    table.place(new, Descriptor::for_file(file));
    Ok(new.into())
}

pub(crate) fn readlink(
    table: &Descriptors,
    mem: &mut GuestMemory<'_, '_>,
    fd: u32,
    path: GuestPtr<str>,
    buf: GuestPtr<u8>,
    buf_len: types::Size,
) -> Answer<types::Size> {
    if buf_len == 0 {
        return Err(Errno::Inval.into());
    }
    let path = mem.path(path)?;
    let target = retry_eagain(|| vfs::do_readlinkat(table.cred(), &start_of(table, fd), &path))?;
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
    let path = mem.path(path)?;
    Ok(retry_eagain(|| {
        vfs::filename_rmdir(table.cred(), &start_of(table, fd), &path)
    })?)
}

pub(crate) fn rename(
    table: &Descriptors,
    mem: &GuestMemory<'_, '_>,
    old: (u32, GuestPtr<str>),
    new: (u32, GuestPtr<str>),
) -> Answer {
    let (from_path, to_path) = (mem.path(old.1)?, mem.path(new.1)?);
    Ok(retry_eagain(|| {
        let (from, to) = (start_of(table, old.0), start_of(table, new.0));
        vfs::filename_renameat2(table.cred(), &from, &from_path, &to, &to_path)
    })?)
}

pub(crate) fn symlink(
    table: &Descriptors,
    mem: &GuestMemory<'_, '_>,
    old_path: GuestPtr<str>,
    fd: u32,
    new_path: GuestPtr<str>,
) -> Answer {
    let (target, path) = (mem.path(old_path)?, mem.path(new_path)?);
    // wasi-testsuite's symlink_create has a guest make no link to an absolute
    // path, where Linux would make one; a guest walks none of them anyway. It
    // is refused once the start is found, where the link would be made.
    let start = || -> Result<vfs::Path, FilesystemError> {
        let start = start_of(table, fd).path_init()?;
        match target.starts_with('/') {
            true => Err(FilesystemError::NotPermitted),
            false => Ok(start),
        }
    };
    Ok(retry_eagain(|| {
        vfs::filename_symlinkat(table.cred(), &target, &start, &path)
    })?)
}

pub(crate) fn unlink_file(
    table: &Descriptors,
    mem: &GuestMemory<'_, '_>,
    fd: u32,
    path: GuestPtr<str>,
) -> Answer {
    let path = mem.path(path)?;
    Ok(retry_eagain(|| {
        vfs::filename_unlinkat(table.cred(), &start_of(table, fd), &path)
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

    /// What `retry_eagain` answers when the first `moves` calls each had a move
    /// land in them, and how many calls it made.
    fn retried(moves: usize) -> (Result<(), FilesystemError>, usize) {
        let mut made = 0;
        let answer = retry_eagain(|| {
            made += 1;
            match made <= moves {
                true => Err(FilesystemError::WouldBlock),
                false => Ok(()),
            }
        });
        (answer, made)
    }

    // A move lands during a walk only between two of its steps, which no guest
    // call can time, so the retry itself witnesses it.
    // @behavior RT-256
    #[test]
    fn a_path_call_a_move_landed_in_is_made_again_four_times_at_most() {
        assert_eq!(
            [retried(0), retried(3), retried(4)],
            [
                (Ok(()), 1),
                (Ok(()), 4),
                (Err(FilesystemError::WouldBlock), 4),
            ],
            "a path_* call answered EAGAIN must be made again until it answers otherwise, four \
             times in all at most, as cap-std makes openat2"
        );
    }
}
