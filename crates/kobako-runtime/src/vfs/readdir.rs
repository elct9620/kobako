//! Listing a directory, as Linux's `fs/readdir.c` hands each entry a
//! Filesystem emits to the actor a listing names, with the helpers `libfs.c`
//! gives every Filesystem.

use std::sync::Arc;

use super::dentry::Dentry;
use super::inode::Identity;
use super::{Errno, FileType, Kstat, PATH_MAX};
use crate::cred::Cred;

#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct DirEntry {
    pub name: String,
    pub file_type: FileType,
    /// The device and inode numbers naming the entry, as `Kstat` reports
    /// them and `dir_emit` hands `d_ino`. An `ino` of zero reports none.
    pub dev: u64,
    pub ino: u64,
}

impl DirEntry {
    /// The entry `name` in its directory, naming a `file_type`.
    pub fn new(name: impl Into<String>, file_type: FileType) -> Self {
        Self {
            name: name.into(),
            file_type,
            dev: 0,
            ino: 0,
        }
    }

    /// This entry naming `dev` and `ino`, as a stat of it reports them.
    pub fn with_identity(self, dev: u64, ino: u64) -> Self {
        Self { dev, ino, ..self }
    }
}

/// One entry a listing hands its actor, as `dir_emit` does.
#[non_exhaustive]
pub struct DirEmit<'a> {
    pub name: &'a str,
    pub file_type: FileType,
    /// What the entry is, `None` where its Filesystem names no inode number.
    pub identity: Option<Identity>,
    /// Where the entry stands, as `dir_emit` hands `ctx->pos`, so a listing
    /// from there reaches it first.
    pub offset: u64,
}

impl DirEmit<'_> {
    /// This entry as a list keeps one.
    pub fn to_dir_entry(&self) -> DirEntry {
        let entry = DirEntry::new(self.name, self.file_type);
        match self.identity {
            Some(identity) => {
                let named = identity.onto(Kstat::new(self.file_type, 0));
                entry.with_identity(named.dev, named.ino)
            }
            None => entry,
        }
    }
}

/// A listing under way, as Linux's `struct dir_context`: where it stands, who
/// takes each entry, answering whether to go on, and who the listing is for.
pub struct DirContext<'a> {
    pub(super) pos: u64,
    actor: &'a mut dyn FnMut(&DirEmit<'_>) -> bool,
    pub(super) cred: Arc<Cred>,
}

impl<'a> DirContext<'a> {
    /// A listing from `pos`, each entry going to `actor`. A listing of an open
    /// directory starts where that directory stands instead.
    pub fn new(pos: u64, actor: &'a mut dyn FnMut(&DirEmit<'_>) -> bool) -> Self {
        Self {
            pos,
            actor,
            cred: Cred::new(Vec::new()),
        }
    }

    /// Where the listing stands, as `ctx->pos`.
    pub fn pos(&self) -> u64 {
        self.pos
    }

    /// Who the listing is for, as `file->f_cred`.
    pub fn cred(&self) -> &Arc<Cred> {
        &self.cred
    }

    /// Move the listing to `pos`, as a Filesystem sets `ctx->pos` to where its
    /// next entry stands, or past its last.
    pub fn set_pos(&mut self, pos: u64) {
        self.pos = pos;
    }

    /// Hand the actor `entry`, standing where the listing stands, answering
    /// whether to go on, as `dir_emit` does. The Filesystem moves the listing
    /// past what the actor took. The actor judges the name, as `filldir64` asks
    /// `verify_dirent_name`.
    pub fn dir_emit(&mut self, entry: &DirEntry) -> bool {
        let identity = Identity::from_numbers(entry.dev, entry.ino);
        self.emit(&entry.name, entry.file_type, identity)
    }

    fn emit(&mut self, name: &str, file_type: FileType, identity: Option<Identity>) -> bool {
        (self.actor)(&DirEmit {
            name,
            file_type,
            identity,
            offset: self.pos,
        })
    }
}

/// The identity the inode `dentry` holds was looked up with, as Linux reads
/// `i_ino`.
fn i_ino(dentry: &Dentry) -> Option<Identity> {
    dentry.inode().and_then(|inode| inode.identity())
}

/// Hand `ctx` the `.` and `..` it has yet to reach, as Linux's `dir_emit_dots`
/// does with the inode numbers the dentries hold, `..` naming what stands above
/// `dentry`, or itself at the root. It asks no Filesystem and answers whether
/// to go on.
pub fn dir_emit_dots(dentry: &Dentry, ctx: &mut DirContext<'_>) -> bool {
    if ctx.pos == 0 {
        if !ctx.emit(".", FileType::Dir, i_ino(dentry)) {
            return false;
        }
        ctx.pos = 1;
    }
    if ctx.pos == 1 {
        if !ctx.emit("..", FileType::Dir, i_ino(&dentry.parent())) {
            return false;
        }
        ctx.pos = 2;
    }
    true
}

/// What a directory's read answers, as Linux's `generic_read_dir` is every
/// directory's `f_op->read`.
pub fn generic_read_dir() -> Result<usize, Errno> {
    Err(Errno::IsADirectory)
}

/// Refuse a name no directory can hold, as Linux's `verify_dirent_name` refuses
/// what a Filesystem hands it, so whoever walks a listing is never handed a
/// path where it was promised one name. A Filesystem that asks about each name
/// before it lists them asks this first.
pub fn verify_dirent_name(name: &str) -> Result<(), Errno> {
    if name.is_empty() || name.len() >= PATH_MAX || name.contains('/') {
        return Err(Errno::Other(
            "a Filesystem listed a name no directory can hold".to_string(),
        ));
    }
    Ok(())
}
