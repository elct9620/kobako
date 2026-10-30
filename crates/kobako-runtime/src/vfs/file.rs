//! An open file, as Linux's `struct file`: where it was opened, what answers
//! for it, and what each descriptor of it shares, so any ABI a Driver encodes
//! holds the same open file.

use super::namei::down_read;
use super::openat2::OpenHow;
use super::path::Path;
use super::Error;
use crate::filesystem::{DirEntry, FileOperations, FilesystemError, Metadata, OpenOptions};
use crate::mount::Mount;
use crate::profile::Profile;

/// One open file or directory, as a descriptor refers to it.
pub struct File {
    /// Where it was opened, as `f_path`; it follows the name wherever it
    /// moves.
    path: Path,
    /// What answers for it, as `f_op`.
    ops: Box<dyn FileOperations>,
    directory: bool,
    /// What it was opened for, as `f_mode`.
    readable: bool,
    writable: bool,
    /// Whether each write lands at the end, as `O_APPEND` in `f_flags`.
    append: bool,
    /// Where the next read or write lands, as `f_pos`.
    pos: u64,
    /// The entries a listing began with, held until it reaches its end, so
    /// each later step reads on without listing the directory again.
    listing: Option<Vec<DirEntry>>,
}

impl File {
    /// The file `ops` answers for, opened at `path` as `how` asks.
    pub(super) fn new(path: Path, ops: Box<dyn FileOperations>, how: &OpenHow) -> Self {
        Self {
            path,
            ops,
            directory: false,
            readable: how.read,
            writable: how.write,
            append: how.append,
            pos: 0,
            listing: None,
        }
    }

    /// The directory at `path`, opened to be listed, as `dentry_open` calls
    /// `f_op->open`. A directory without `open` opens as Linux's opens with
    /// no `f_op->open`, and refuses a listing as one with no
    /// `iterate_shared`.
    pub(super) fn directory(path: Path) -> Result<Self, FilesystemError> {
        struct Unlistable;
        impl FileOperations for Unlistable {}

        let options = OpenOptions {
            read: true,
            directory: true,
            ..OpenOptions::default()
        };
        let ops = match path.dir().open(path.dentry(), &options) {
            Some(opened) => opened?,
            None => Box::new(Unlistable),
        };
        Ok(Self {
            directory: true,
            ..Self::new(path, ops, &OpenHow::default())
        })
    }

    /// The root of `mount` opened as a directory, for an invocation running
    /// under `profile`.
    pub fn root(mount: Mount, profile: Profile) -> Result<Self, FilesystemError> {
        Self::directory(Path::root(mount, profile))
    }

    /// Where it was opened; a call relative to it starts here.
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn is_directory(&self) -> bool {
        self.directory
    }

    /// Whether it was opened to read.
    pub fn readable(&self) -> bool {
        self.readable
    }

    /// Whether it was opened to write.
    pub fn writable(&self) -> bool {
        self.writable
    }

    /// Whether each write lands at the end.
    pub fn append(&self) -> bool {
        self.append
    }

    pub fn set_append(&mut self, append: bool) {
        self.append = append;
    }

    /// Where the next read or write lands.
    pub fn pos(&self) -> u64 {
        self.pos
    }

    pub fn set_pos(&mut self, pos: u64) {
        self.pos = pos;
    }

    /// What answers for it.
    pub fn ops(&mut self) -> &mut dyn FileOperations {
        self.ops.as_mut()
    }

    /// What it reports of itself, as `vfs_getattr` answers.
    pub fn getattr(&self) -> Result<Metadata, Error> {
        match self.directory {
            true => self.path.metadata(),
            false => Ok(self.ops.getattr()?),
        }
    }

    /// The entries of the directory, without `.` or `..`; none once it was
    /// removed, as Linux lists a dead directory.
    pub fn iterate_dir(&mut self) -> Result<Vec<DirEntry>, Error> {
        self.path.revalidate_path()?;
        let dentry = self.path.dentry().clone();
        // A listing holds the directory shared, as Linux's `iterate_dir`
        // does, so a removal through the VFS lands before or after it.
        let _held = down_read(self.path.mount(), &dentry)?;
        if dentry.is_dead() {
            return Ok(Vec::new());
        }
        Ok(self.ops.iterate_shared(&dentry)?)
    }

    /// The entries a listing began with, once one is under way.
    pub fn take_listing(&mut self) -> Option<Vec<DirEntry>> {
        self.listing.take()
    }

    /// Hold the entries a listing began with for its next step.
    pub fn keep_listing(&mut self, listing: Vec<DirEntry>) {
        self.listing = Some(listing);
    }

    /// Release it once the last descriptor closes, as `filp_close` does.
    pub fn release(self) -> Result<(), FilesystemError> {
        self.ops.release()
    }
}
