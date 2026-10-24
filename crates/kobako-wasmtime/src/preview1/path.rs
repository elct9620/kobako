//! The 10 `path_*` calls. Each resolves a path beneath a directory
//! descriptor, asks its Mount what the guest may do there, and asks the
//! Filesystem. Links and timestamps are no Filesystem's, so those calls
//! are unsupported.

use std::sync::Arc;

use kobako_runtime::filesystem::{Access, Kind, OpenOptions, Rename};
use kobako_runtime::mount::Mount;

use super::args::{self, FDFLAGS_APPEND, FDFLAGS_SYNCHRONIZED, RIGHTS_FD_READ, RIGHTS_FD_WRITE};
use super::beneath::{resolve, ResolvedPath};
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
fn directory(table: &Descriptors, fd: u32) -> Answer<&OpenDirectory> {
    match table.get(fd).ok_or(Errno::Badf)? {
        Descriptor::Directory(dir) => Ok(dir),
        Descriptor::Stdin(_) | Descriptor::Channel(_) | Descriptor::File(_) => {
            Err(Errno::Badf.into())
        }
    }
}

/// Whether the guest may reach `path` with `access`, as its Mount
/// decides. A Mount that follows no link refuses one standing on the way,
/// and at the end too when the call would follow it.
fn judge(mount: &Mount, path: &str, access: Access, follows_last: bool) -> Answer {
    mount.check(path, access)?;
    if mount.follows_links() {
        return Ok(());
    }
    let ends = path.match_indices('/').map(|(end, _)| end);
    for end in ends.chain(follows_last.then_some(path.len())) {
        match mount.filesystem().symlink_metadata(&path[..end]) {
            Ok(metadata) if metadata.kind == Kind::Symlink => return Err(Errno::Loop.into()),
            Ok(_) => {}
            // What is missing holds no link; the call answers for it.
            Err(_) => break,
        }
    }
    Ok(())
}

/// A path beneath `fd`, relative to its mount's root, refused when it
/// ends in `/` and names a file.
fn beneath<'a>(
    table: &'a Descriptors,
    mem: &GuestMemory,
    fd: u32,
    path: (u32, u32),
) -> Answer<(&'a OpenDirectory, String)> {
    let dir = directory(table, fd)?;
    let path = resolve(dir, &mem.path(path.0, path.1)?)?
        .check_directory(dir.mount.filesystem().as_ref())?;
    Ok((dir, path))
}

/// A trailing `/` asks for the directory being made, so it is no
/// refusal here, as `mkdir(2)` takes it.
pub(crate) fn create_directory(
    table: &Descriptors,
    mem: &GuestMemory,
    fd: u32,
    path: (u32, u32),
) -> Answer {
    let dir = directory(table, fd)?;
    let path = resolve(dir, &mem.path(path.0, path.1)?)?.path;
    judge(&dir.mount, &path, Access::ReadWrite, false)?;
    Ok(dir.mount.filesystem().create_dir(&path)?)
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
    let resolved = resolve(dir, &mem.path(path.0, path.1)?)?;
    // A trailing `/` resolves a link standing there, as Linux's `lstat` does.
    let follow = flags & LOOKUPFLAGS_SYMLINK_FOLLOW != 0 || resolved.names_directory;
    let filesystem = dir.mount.filesystem();
    let path = resolved.check_directory(filesystem.as_ref())?;
    let metadata = if follow {
        filesystem.metadata(&path)?
    } else {
        filesystem.symlink_metadata(&path)?
    };
    write_metadata(mem, out, metadata)
}

/// A Filesystem keeps no timestamps of the guest's choosing.
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
    beneath(table, mem, fd, path)?;
    Err(Errno::Notsup.into())
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
        Descriptor::Directory(dir) => dir,
        Descriptor::File(_) => return Err(Errno::Notdir.into()),
        Descriptor::Stdin(_) | Descriptor::Channel(_) => return Err(Errno::Badf.into()),
    };
    let oflag = |bit| flags.oflags & bit != 0;
    // A Filesystem makes no write durable on its own schedule, so an open
    // asking it to is unsupported rather than quietly granted.
    if flags.fdflags & FDFLAGS_SYNCHRONIZED != 0 {
        return Err(Errno::Notsup.into());
    }
    if oflag(OFLAGS_DIRECTORY) && oflag(OFLAGS_CREAT | OFLAGS_EXCL | OFLAGS_TRUNC) {
        return Err(Errno::Inval.into());
    }
    let resolved = resolve(dir, &path)?;
    // A path ending in `/` asks for a directory, which cannot be created
    // as a file.
    if path.ends_with('/') && oflag(OFLAGS_CREAT) {
        return Err(Errno::Isdir.into());
    }
    // A trailing `/` resolves a link standing there, as Linux's `open` does.
    let follow = flags.dirflags & LOOKUPFLAGS_SYMLINK_FOLLOW != 0 || resolved.names_directory;
    let path = resolved.check_directory(dir.mount.filesystem().as_ref())?;
    let mount = Arc::clone(&dir.mount);
    if table.is_full() {
        return Err(Errno::Mfile.into());
    }

    let read = flags.rights_base & RIGHTS_FD_READ != 0;
    let write = flags.rights_base & RIGHTS_FD_WRITE != 0;
    // Creating changes the Mount only where nothing stands yet, so an open
    // that may create is judged as a read until that is known.
    let changes = write || oflag(OFLAGS_TRUNC);
    let access = if changes {
        Access::ReadWrite
    } else {
        Access::Read
    };
    judge(&mount, &path, access, follow)?;

    let looked_up = if follow {
        mount.filesystem().metadata(&path)
    } else {
        mount.filesystem().symlink_metadata(&path)
    };
    let existing = match looked_up {
        Ok(metadata) => Some(metadata.kind),
        Err(kobako_runtime::filesystem::FilesystemError::NotFound) => None,
        Err(err) => return Err(err.into()),
    };
    if existing.is_none() && oflag(OFLAGS_CREAT) && !changes {
        judge(&mount, &path, Access::ReadWrite, follow)?;
    }
    let descriptor = match existing {
        Some(_) if oflag(OFLAGS_CREAT) && oflag(OFLAGS_EXCL) => return Err(Errno::Exist.into()),
        Some(Kind::Directory) if write || oflag(OFLAGS_TRUNC | OFLAGS_CREAT) => {
            return Err(Errno::Isdir.into())
        }
        Some(Kind::Directory) => Descriptor::Directory(OpenDirectory {
            mount,
            path,
            preopen: false,
            listing: None,
        }),
        Some(_) if oflag(OFLAGS_DIRECTORY) => return Err(Errno::Notdir.into()),
        None if oflag(OFLAGS_DIRECTORY) || !oflag(OFLAGS_CREAT) => return Err(Errno::Noent.into()),
        Some(_) | None => {
            let options = OpenOptions {
                read,
                write,
                // A file already there is opened as it is, so a host that
                // opens only to create never asks for more than the guest.
                create: oflag(OFLAGS_CREAT) && existing.is_none(),
                create_new: oflag(OFLAGS_CREAT) && oflag(OFLAGS_EXCL),
                truncate: oflag(OFLAGS_TRUNC),
                nofollow: !follow,
            };
            Descriptor::File(OpenFile {
                file: mount.filesystem().open(&path, &options)?,
                position: 0,
                append: flags.fdflags & FDFLAGS_APPEND != 0,
                readable: read,
                writable: write,
            })
        }
    };
    let opened = table.push(descriptor).ok_or(Errno::Mfile)?;
    mem.write_u32(out, opened)
}

/// A Filesystem holds no links to read.
pub(crate) fn readlink(
    table: &Descriptors,
    mem: &GuestMemory,
    fd: u32,
    path: (u32, u32),
) -> Answer {
    beneath(table, mem, fd, path)?;
    Err(Errno::Notsup.into())
}

/// Refuse a guest path ending in `.` or `..` as `refusal`, since it names
/// no entry a call may remove or move. Linux looks up what stands before
/// it first, so a missing or non-directory parent answers as itself.
fn refuse_dot_end(dir: &OpenDirectory, path: &str, refusal: Errno) -> Answer {
    let trimmed = path.trim_end_matches('/');
    let (parent, last) = trimmed.rsplit_once('/').unwrap_or(("", trimmed));
    if !matches!(last, "." | "..") {
        return Ok(());
    }
    if !parent.is_empty() {
        let parent = resolve(dir, parent)?.path;
        if dir.mount.filesystem().metadata(&parent)?.kind != Kind::Directory {
            return Err(Errno::Notdir.into());
        }
    }
    Err(refusal.into())
}

/// A Mount's root is where the guest stands, not a directory it may
/// remove or move; each call refuses it as Linux refuses its own root.
fn refuse_root(path: &str, refusal: Errno) -> Answer {
    if path.is_empty() {
        return Err(refusal.into());
    }
    Ok(())
}

pub(crate) fn remove_directory(
    table: &Descriptors,
    mem: &GuestMemory,
    fd: u32,
    path: (u32, u32),
) -> Answer {
    refuse_dot_end(
        directory(table, fd)?,
        &mem.path(path.0, path.1)?,
        Errno::Inval,
    )?;
    let (dir, path) = beneath(table, mem, fd, path)?;
    refuse_root(&path, Errno::Inval)?;
    judge(&dir.mount, &path, Access::ReadWrite, false)?;
    Ok(dir.mount.filesystem().remove_dir(&path)?)
}

/// A rename stays within one Mount; across two it is `XDEV`, as
/// across two mounted devices. A directory moves only beneath a Mount
/// that allows it, the Filesystem checking what it moves as it moves it.
pub(crate) fn rename(
    table: &Descriptors,
    mem: &GuestMemory,
    old: (u32, (u32, u32)),
    new: (u32, (u32, u32)),
) -> Answer {
    let from_dir = directory(table, old.0)?;
    let to_dir = directory(table, new.0)?;
    let (from, to) = (mem.path(old.1 .0, old.1 .1)?, mem.path(new.1 .0, new.1 .1)?);
    refuse_dot_end(from_dir, &from, Errno::Busy)?;
    refuse_dot_end(to_dir, &to, Errno::Busy)?;
    let from = resolve(from_dir, &from)?;
    let to = resolve(to_dir, &to)?;
    refuse_root(&from.path, Errno::Busy)?;
    refuse_root(&to.path, Errno::Busy)?;
    // A trailing `/` on either side asks that the source be a directory,
    // which `rename(2)` checks before it finds a rename onto its own path
    // leaves everything as it was.
    let names_directory = from.names_directory || to.names_directory;
    let from = ResolvedPath {
        names_directory,
        ..from
    }
    .check_directory(from_dir.mount.filesystem().as_ref())?;
    let to = match to {
        to if to.path == from => to.path,
        to => to.check_directory(to_dir.mount.filesystem().as_ref())?,
    };
    if !Arc::ptr_eq(&from_dir.mount, &to_dir.mount) {
        return Err(Errno::Xdev.into());
    }
    judge(&from_dir.mount, &from, Access::ReadWrite, false)?;
    judge(&to_dir.mount, &to, Access::ReadWrite, false)?;
    let rename = match (from_dir.mount.moves_directories(), names_directory) {
        (false, _) => Rename::File,
        (true, true) => Rename::Directory,
        (true, false) => Rename::Any,
    };
    Ok(from_dir.mount.filesystem().rename(&from, &to, rename)?)
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
    let (dir, path) = beneath(table, mem, fd, path)?;
    refuse_root(&path, Errno::Isdir)?;
    judge(&dir.mount, &path, Access::ReadWrite, false)?;
    Ok(dir.mount.filesystem().remove_file(&path)?)
}
