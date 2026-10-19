//! The 10 `path_*` calls. Each resolves a path beneath a directory
//! descriptor, asks its Mount what the guest may do there, and asks the
//! Filesystem. Links and timestamps are no Filesystem's, so those calls
//! are unsupported.

use std::sync::Arc;

use kobako_runtime::filesystem::{Kind, OpenOptions, Rename};
use kobako_runtime::mount::Access;

use super::args::{self, FDFLAGS_APPEND, FDFLAGS_SYNCHRONIZED, RIGHTS_FD_READ, RIGHTS_FD_WRITE};
use super::beneath::{resolve, Resolved};
use super::errno::{Answer, Errno};
use super::fd::write_metadata;
use super::memory::GuestMemory;
use crate::descriptors::{Descriptor, Descriptors, Directory, OpenFile};

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
fn directory(table: &Descriptors, fd: u32) -> Answer<&Directory> {
    match table.get(fd).ok_or(Errno::Badf)? {
        Descriptor::Directory(dir) => Ok(dir),
        Descriptor::Stdin(_) | Descriptor::Channel(_) | Descriptor::File(_) => {
            Err(Errno::Badf.into())
        }
    }
}

/// A path beneath `fd`, relative to its mount's root, refused when it
/// ends in `/` and names a file.
fn beneath<'a>(
    table: &'a Descriptors,
    mem: &GuestMemory,
    fd: u32,
    path: (u32, u32),
) -> Answer<(&'a Directory, String)> {
    let dir = directory(table, fd)?;
    let path = resolve(dir, &mem.path(path.0, path.1)?)?
        .directory_checked(dir.mount.filesystem().as_ref())?;
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
    dir.mount.check(&path, Access::ReadWrite)?;
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
    let (dir, path) = beneath(table, mem, fd, path)?;
    let metadata = dir.mount.filesystem().metadata(&path)?;
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
    let path = resolved.directory_checked(dir.mount.filesystem().as_ref())?;
    let mount = Arc::clone(&dir.mount);
    if table.is_full() {
        return Err(Errno::Mfile.into());
    }

    let read = flags.rights_base & RIGHTS_FD_READ != 0;
    let write = flags.rights_base & RIGHTS_FD_WRITE != 0;
    let changes = write || oflag(OFLAGS_CREAT) || oflag(OFLAGS_TRUNC);
    let access = if changes {
        Access::ReadWrite
    } else {
        Access::Read
    };
    mount.check(&path, access)?;

    let existing = match mount.filesystem().metadata(&path) {
        Ok(metadata) => Some(metadata.kind),
        Err(kobako_runtime::filesystem::FilesystemError::NotFound) => None,
        Err(err) => return Err(err.into()),
    };
    let descriptor = match existing {
        Some(_) if oflag(OFLAGS_CREAT) && oflag(OFLAGS_EXCL) => return Err(Errno::Exist.into()),
        Some(Kind::Directory) if write || oflag(OFLAGS_TRUNC) => return Err(Errno::Isdir.into()),
        Some(Kind::Directory) => Descriptor::Directory(Directory {
            mount,
            path,
            preopen: false,
        }),
        Some(_) if oflag(OFLAGS_DIRECTORY) => return Err(Errno::Notdir.into()),
        None if oflag(OFLAGS_DIRECTORY) || !oflag(OFLAGS_CREAT) => return Err(Errno::Noent.into()),
        Some(_) | None => {
            let options = OpenOptions {
                read,
                write,
                create: oflag(OFLAGS_CREAT),
                create_new: oflag(OFLAGS_CREAT) && oflag(OFLAGS_EXCL),
                truncate: oflag(OFLAGS_TRUNC),
                nofollow: flags.dirflags & LOOKUPFLAGS_SYMLINK_FOLLOW == 0,
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

/// A Mount's root is where the guest stands, not a directory it may
/// remove or move; each call refuses it as Linux refuses its own root.
fn not_root(path: &str, refusal: Errno) -> Answer {
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
    let (dir, path) = beneath(table, mem, fd, path)?;
    not_root(&path, Errno::Inval)?;
    dir.mount.check(&path, Access::ReadWrite)?;
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
    let from = resolve(from_dir, &mem.path(old.1 .0, old.1 .1)?)?;
    let to = resolve(to_dir, &mem.path(new.1 .0, new.1 .1)?)?;
    not_root(&from.path, Errno::Busy)?;
    not_root(&to.path, Errno::Busy)?;
    // A trailing `/` on either side asks that the source be a directory,
    // which `rename(2)` checks before it finds a rename onto its own path
    // leaves everything as it was.
    let names_directory = from.names_directory || to.names_directory;
    let from = Resolved {
        names_directory,
        ..from
    }
    .directory_checked(from_dir.mount.filesystem().as_ref())?;
    let to = match to {
        to if to.path == from => to.path,
        to => to.directory_checked(to_dir.mount.filesystem().as_ref())?,
    };
    if !Arc::ptr_eq(&from_dir.mount, &to_dir.mount) {
        return Err(Errno::Xdev.into());
    }
    from_dir.mount.check(&from, Access::ReadWrite)?;
    to_dir.mount.check(&to, Access::ReadWrite)?;
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
    not_root(&path, Errno::Isdir)?;
    dir.mount.check(&path, Access::ReadWrite)?;
    Ok(dir.mount.filesystem().remove_file(&path)?)
}
