//! Making, removing, and moving an entry, as Linux's `mkdirat`,
//! `unlinkat`, and `renameat2` do.

use std::sync::Arc;

use super::namei::resolve;
use super::path::Path;
use super::Error;
use crate::filesystem::{Access, FileType, FilesystemError, Rename};

/// Make the directory `path` beneath `start`. A trailing `/` asks for the
/// directory being made, so it is no refusal here, as `mkdir(2)` takes it.
pub fn create_dir(start: &Path, path: &str) -> Result<(), Error> {
    let resolved = resolve(start, path, false, false)?;
    resolved.judge(Access::ReadWrite)?;
    Ok(resolved.base.create_dir(&resolved.name)?)
}

/// Remove the empty directory `path` beneath `start`.
pub fn remove_dir(start: &Path, path: &str) -> Result<(), Error> {
    refuse_dot_end(start, path, FilesystemError::InvalidInput)?;
    let mut resolved = resolve(start, path, false, false)?;
    resolved.judge(Access::ReadWrite)?;
    resolved.check_directory()?;
    refuse_root(&resolved.name, FilesystemError::InvalidInput)?;
    resolved.base.remove_dir(&resolved.name)?;
    resolved
        .mount
        .super_block()
        .dentries()
        .delete(&resolved.dentry, &resolved.name);
    Ok(())
}

/// Remove the file `path` beneath `start`. Linux's `unlink` follows no
/// link at the end, so a trailing `/` is answered by the name itself: no
/// file removes as a directory.
pub fn remove_file(start: &Path, path: &str) -> Result<(), Error> {
    let mut resolved = resolve(start, path, false, false)?;
    resolved.judge(Access::ReadWrite)?;
    resolved.check_directory()?;
    refuse_root(&resolved.name, FilesystemError::IsADirectory)?;
    Ok(resolved.base.remove_file(&resolved.name)?)
}

/// Move `from_path` beneath `from` to `to_path` beneath `to`. A rename
/// stays within one Mount; across two it crosses devices, as across two
/// mounted devices. A directory moves only beneath a Mount that allows it,
/// the Filesystem checking what it moves as it moves it.
pub fn rename(from: &Path, from_path: &str, to: &Path, to_path: &str) -> Result<(), Error> {
    refuse_dot_end(from, from_path, FilesystemError::ResourceBusy)?;
    refuse_dot_end(to, to_path, FilesystemError::ResourceBusy)?;
    let mut source = resolve(from, from_path, false, false)?;
    let mut target = resolve(to, to_path, false, false)?;
    refuse_root(&source.name, FilesystemError::ResourceBusy)?;
    refuse_root(&target.name, FilesystemError::ResourceBusy)?;
    // A trailing `/` on either side asks that the source be a directory,
    // which `rename(2)` checks before it finds a rename onto its own path
    // leaves everything as it was. A rename moves a link rather than what
    // it points at, so each side is checked as it stands.
    let names_directory = source.names_directory || target.names_directory;
    let onto_itself = target.judged == source.judged;
    source.names_directory = names_directory;
    source.check_directory()?;
    if !onto_itself {
        target.check_directory()?;
    }
    if !Arc::ptr_eq(from.mount(), to.mount()) {
        return Err(FilesystemError::CrossesDevices.into());
    }
    source.judge(Access::ReadWrite)?;
    target.judge(Access::ReadWrite)?;
    // Each side was walked on its own, so a rename onto its own path is
    // known by the path rather than by the directories each walk opened;
    // it changes nothing once the source stands, as `rename(2)` answers.
    if onto_itself {
        source.found()?;
        return Ok(());
    }
    let rename = match (from.mount().moves_directories(), names_directory) {
        (false, _) => Rename::File,
        (true, true) => Rename::Directory,
        (true, false) => Rename::Any,
    };
    source
        .base
        .rename(&source.name, target.base.as_ref(), &target.name, rename)?;
    // A directory replaced at the target is gone for whoever holds it, and
    // one moved from the source is reached where it went.
    let dentries = source.mount.super_block().dentries();
    dentries.delete(&target.dentry, &target.name);
    dentries.rename(&source.dentry, &source.name, &target.dentry, &target.name);
    Ok(())
}

/// Refuse a guest path ending in `.` or `..` as `refusal`, since it names
/// no entry a call may remove or move. Linux looks up what stands before
/// it first, so a missing or non-directory parent answers as itself.
fn refuse_dot_end(start: &Path, path: &str, refusal: FilesystemError) -> Result<(), Error> {
    let trimmed = path.trim_end_matches('/');
    let (parent, last) = trimmed.rsplit_once('/').unwrap_or(("", trimmed));
    if !matches!(last, "." | "..") {
        return Ok(());
    }
    if !parent.is_empty()
        && resolve(start, parent, false, false)?.found()?.file_type != FileType::Directory
    {
        return Err(FilesystemError::NotADirectory.into());
    }
    Err(refusal.into())
}

/// A Mount's root is where the guest stands, not a directory it may
/// remove or move; each call refuses it as Linux refuses its own root.
fn refuse_root(name: &str, refusal: FilesystemError) -> Result<(), Error> {
    if name.is_empty() {
        return Err(refusal.into());
    }
    Ok(())
}
