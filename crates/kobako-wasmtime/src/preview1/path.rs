//! The 10 `path_*` calls. Each resolves a path beneath a directory
//! descriptor, asks its Mount what the guest may do there, and asks the
//! Filesystem. Links and timestamps are no Filesystem's, so those calls
//! are unsupported.

use std::sync::Arc;

use kobako_runtime::filesystem::{
    Access, FileType, Filesystem, FilesystemError, OpenOptions, Rename,
};
use kobako_runtime::mount::Mount;

use super::args::{
    self, FDFLAGS_APPEND, FDFLAGS_NONBLOCK, FDFLAGS_SYNCHRONIZED, RIGHTS_FD_READ, RIGHTS_FD_WRITE,
};
use super::beneath::{resolve, ResolvedPath};
use super::errno::{Answer, Errno};
use super::fd::write_metadata;
use super::memory::GuestMemory;
use crate::descriptors::{Descriptor, Descriptors, Identity, OpenDirectory, OpenFile};

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
/// decides. A Mount that follows no link refuses one first. A Mount with
/// a Policy, the one kind judging a path at all, resolves every path from
/// its root, so `path` is the one the Policy judges.
fn judge(mount: &Mount, path: &str, access: Access, follows_last: bool) -> Answer {
    if !mount.follows_links() {
        refuse_links(mount, path, follows_last)?;
    }
    Ok(mount.check(path, access)?)
}

/// Refuse a link standing on the way to `path`, and at its end when the
/// call would follow it, before the Mount judges a path the link would
/// lead elsewhere.
fn refuse_links(mount: &Mount, path: &str, follows_last: bool) -> Answer {
    let ends = path.match_indices('/').map(|(end, _)| end);
    for end in ends.chain(follows_last.then_some(path.len())) {
        match mount.filesystem().symlink_metadata(&path[..end]) {
            Ok(metadata) if metadata.file_type == FileType::Symlink => {
                return Err(Errno::Loop.into())
            }
            Ok(_) => {}
            // What is missing holds no link; the call answers for it.
            Err(FilesystemError::NotFound | FilesystemError::NotADirectory) => break,
            Err(err) => return Err(err.into()),
        }
    }
    Ok(())
}

/// A path beneath `fd` and the directory it is relative to, for a call
/// acting on what stands there rather than where a link leads: refused
/// when it ends in `/` and names a file or a link.
fn beneath<'a>(
    table: &'a Descriptors,
    mem: &GuestMemory,
    fd: u32,
    path: (u32, u32),
) -> Answer<(&'a OpenDirectory, Arc<dyn Filesystem>, String)> {
    let dir = directory(table, fd)?;
    let (base, path) = resolve(dir, &mem.path(path.0, path.1)?)?
        .check_directory(<dyn Filesystem>::symlink_metadata)?;
    Ok((dir, base, path))
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
    let resolved = resolve(dir, &mem.path(path.0, path.1)?)?;
    judge(&dir.mount, &resolved.path, Access::ReadWrite, false)?;
    Ok(resolved.base.create_dir(&resolved.path)?)
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
    let resolved = if follow {
        resolved.follow_last(dir, false)?
    } else {
        resolved
    };
    let (filesystem, path) = resolved.check_directory(<dyn Filesystem>::metadata)?;
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
    // A Filesystem makes no write durable on its own schedule, and every
    // call waits for its answer, so an open asking otherwise is
    // unsupported rather than quietly granted.
    if flags.fdflags & (FDFLAGS_SYNCHRONIZED | FDFLAGS_NONBLOCK) != 0 {
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
    let resolved = if follow {
        resolved.follow_last(dir, oflag(OFLAGS_CREAT))?
    } else {
        resolved
    };
    let (base, path) = resolved.check_directory(<dyn Filesystem>::metadata)?;
    let mount = Arc::clone(&dir.mount);
    // Beneath a Policy the path is already the Mount's; elsewhere it is
    // kept as the opened directory's path at the time.
    let opened_at = if mount.follows_links() {
        join(&dir.path, &path)
    } else {
        path.clone()
    };
    if table.is_full() {
        return Err(Errno::Mfile.into());
    }

    let read = flags.rights_base & RIGHTS_FD_READ != 0;
    let write = flags.rights_base & RIGHTS_FD_WRITE != 0;
    if !mount.follows_links() {
        refuse_links(&mount, &path, follow)?;
    }
    let looked_up = if follow {
        base.metadata(&path)
    } else {
        base.symlink_metadata(&path)
    };
    let (existing, identity) = match looked_up {
        Ok(metadata) => (Some(metadata.file_type), Identity::of(&metadata)),
        Err(FilesystemError::NotFound) => (None, None),
        Err(err) => return Err(err.into()),
    };
    // What stands at the path answers before any access is judged, as
    // Linux's `open` does on a read-only mount.
    let refusal = match existing {
        None if !oflag(OFLAGS_CREAT) => Some(Errno::Noent),
        Some(_) if oflag(OFLAGS_CREAT) && oflag(OFLAGS_EXCL) => Some(Errno::Exist),
        Some(FileType::Directory) if write || oflag(OFLAGS_TRUNC | OFLAGS_CREAT) => {
            Some(Errno::Isdir)
        }
        Some(FileType::Directory) | None => None,
        Some(_) if oflag(OFLAGS_DIRECTORY) => Some(Errno::Notdir),
        Some(_) => None,
    };
    if let Some(errno) = refusal {
        return Err(errno.into());
    }
    // Creating changes the Mount only where nothing stands yet, so the
    // Mount judges the open once, knowing what it does.
    let changes = write || oflag(OFLAGS_TRUNC) || (oflag(OFLAGS_CREAT) && existing.is_none());
    let access = if changes {
        Access::ReadWrite
    } else {
        Access::Read
    };
    mount.check(&path, access)?;
    let descriptor = match existing {
        Some(FileType::Directory) => Descriptor::Directory(OpenDirectory {
            handle: open_dir(base.as_ref(), &path, follow)?,
            _hold: base.hold_directory(&path)?,
            mount,
            path: opened_at,
            identity,
            preopen: false,
            listing: None,
        }),
        Some(_) | None => {
            let mut options = OpenOptions::default();
            options.read = read;
            options.write = write;
            // A file already there is opened as it is, so a host that opens
            // only to create never asks for more than the guest.
            options.create = oflag(OFLAGS_CREAT) && existing.is_none();
            options.create_new = oflag(OFLAGS_CREAT) && oflag(OFLAGS_EXCL);
            options.truncate = oflag(OFLAGS_TRUNC);
            options.nofollow = !follow;
            Descriptor::File(OpenFile {
                file: base.open(&path, &options)?,
                position: 0,
                append: flags.fdflags & FDFLAGS_APPEND != 0,
                readable: read,
                writable: write,
                mount,
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
        let parent = resolve(dir, parent)?;
        if parent.base.metadata(&parent.path)?.file_type != FileType::Directory {
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
    let (dir, base, path) = beneath(table, mem, fd, path)?;
    refuse_root(&path, Errno::Inval)?;
    judge(&dir.mount, &path, Access::ReadWrite, false)?;
    Ok(base.remove_dir(&path)?)
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
    // leaves everything as it was. A rename moves a link rather than what
    // it points at, so each side is checked as it stands.
    let names_directory = from.names_directory || to.names_directory;
    let (from_base, from) = ResolvedPath {
        names_directory,
        ..from
    }
    .check_directory(<dyn Filesystem>::symlink_metadata)?;
    let (to_base, to) = match to {
        to if Arc::ptr_eq(&to.base, &from_base) && to.path == from => (to.base, to.path),
        to => to.check_directory(<dyn Filesystem>::symlink_metadata)?,
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
    Ok(from_base.rename(&from, to_base.as_ref(), &to, rename)?)
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
    // Linux's `unlink` follows no link at the end, so a trailing `/` is
    // answered by the name itself: no file removes as a directory.
    let (dir, base, path) = beneath(table, mem, fd, path)?;
    refuse_root(&path, Errno::Isdir)?;
    judge(&dir.mount, &path, Access::ReadWrite, false)?;
    Ok(base.remove_file(&path)?)
}

/// The directory at `path` beneath `base`, a link standing at its end
/// followed only where `follow` asks, as `O_NOFOLLOW` refuses one.
fn open_dir(base: &dyn Filesystem, path: &str, follow: bool) -> Answer<Arc<dyn Filesystem>> {
    if follow {
        return Ok(base.open_dir(path)?);
    }
    Ok(match path.rsplit_once('/') {
        Some((parent, name)) => base.open_dir(parent)?.open_dir_nofollow(name)?,
        None => base.open_dir_nofollow(path)?,
    })
}

fn join(dir: &str, path: &str) -> String {
    match (dir, path) {
        ("", path) => path.to_string(),
        (dir, "") => dir.to_string(),
        (dir, path) => format!("{dir}/{path}"),
    }
}
