//! The Layers of an Overlay and the rules that make them one view: which
//! Layer shows a name, how a change lands in the upper, and the marks that
//! hide what a lower Layer still holds. The calls on the view are split
//! as overlayfs splits them: finding a name in `namei`, copying up in
//! `copy_up`, listing in `readdir`, and changing a directory in `dir`.
//!
//! Every Layer is asked about a single name in a directory already held, as
//! overlayfs looks a name up in each layer's directory. A Layer's directory
//! merges into the view only beneath a directory it also holds.
//!
//! The upper Tmpfs holds the view's changes in `upper` and builds each one
//! in `work` first, as overlayfs's upperdir and workdir share one mount.
//! A change that replaces an entry is made whole in `work` and exchanged
//! into place, so a change that fails part way leaves the view as it was.

mod copy_up;
mod dir;
mod namei;
mod readdir;

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use super::inode::OverlayDirInode;
use super::path::OverlayPath;

use super::super::{FileOperations, FileType, Filesystem, FilesystemError, Kiocb};
use crate::vfs::{Dentry, Identity};

/// The directories of the upper Tmpfs holding the view's changes and the
/// changes being built, as overlayfs's `upperdir` and `workdir`.
const UPPERDIR: &str = "upper";
const WORKDIR: &str = "work";

/// Where the upper stands among the Layers, topmost first, holding nothing where
/// an Overlay has none, as `ovl_inode` keeps `__upperdentry` apart from its lower
/// stack. Every change lands there.
pub(super) const UPPER: usize = 0;

/// Each Layer's directory at one directory of the view, topmost first;
/// `None` where that Layer holds no directory the view merges there.
pub(super) type Level = Vec<Option<OverlayPath>>;

/// One directory of the view: the dentry the runtime holds for it and the
/// state kept there, as overlayfs reaches a directory by its dentry and
/// the `ovl_inode` behind it.
#[derive(Clone)]
pub(super) struct ViewDir {
    dentry: Arc<Dentry>,
    inode: Arc<OverlayDirInode>,
}

impl ViewDir {
    /// Each Layer's directory this one stands for.
    pub(super) fn level(&self) -> Level {
        self.inode.level()
    }
}

/// What a Layer shows at a name, as `ovl_lookup_single` tells a whiteout from an
/// entry.
enum Presence {
    /// Nothing, and a name too long for that Layer to hold is nothing either, as
    /// `ovl_lookup_single` answers ENOENT and ENAMETOOLONG by moving on.
    Nothing,
    /// A whiteout, which hides the Layers below.
    Whiteout,
    /// An entry, a link reported as itself.
    Entry(LayerEntry),
}

/// An entry a Layer holds: its name as the Layer stores it, and its dentry there.
pub(super) struct LayerEntry {
    stored: String,
    dentry: Arc<Dentry>,
}

impl LayerEntry {
    /// The name as the Layer stores it.
    pub(super) fn stored(&self) -> &str {
        &self.stored
    }

    /// What kind of entry it is, as its lookup recorded it.
    fn kind(&self) -> Result<FileType, FilesystemError> {
        self.dentry.kind()
    }
}

impl Presence {
    fn is_directory(&self) -> bool {
        matches!(self, Self::Entry(found) if found.kind() == Ok(FileType::Dir))
    }
}

/// What `dir` shows at `name`, read from one lookup, as `ovl_lookup_single` reads
/// `ovl_is_whiteout` and `d_is_dir` from the dentry it found.
fn entry(dir: &OverlayPath, name: &str) -> Result<Presence, FilesystemError> {
    match dir.lookup_single(name) {
        Ok(Some((_, dentry))) if dir.is_whiteout_dentry(&dentry) => Ok(Presence::Whiteout),
        Ok(Some((stored, dentry))) => Ok(Presence::Entry(LayerEntry { stored, dentry })),
        Ok(None) | Err(FilesystemError::NotFound | FilesystemError::InvalidFilename) => {
            Ok(Presence::Nothing)
        }
        Err(err) => Err(err),
    }
}

/// Whether a Layer below the upper shows `name` in `level`, as
/// `ovl_lower_positive` finds out whether a removal must leave a whiteout. A Layer
/// that cannot answer is taken to hold something, so nothing below shows through.
fn ovl_lower_positive(level: &Level, name: &str) -> bool {
    for dir in level.iter().skip(UPPER + 1).flatten() {
        match entry(dir, name) {
            Ok(Presence::Nothing) => {}
            Ok(Presence::Whiteout) => return false,
            Ok(Presence::Entry(_)) | Err(_) => return true,
        }
    }
    false
}

fn copy_whole(
    source: &mut dyn FileOperations,
    target: &mut dyn FileOperations,
) -> Result<(), FilesystemError> {
    let mut buf = vec![0; 64 * 1024];
    let (mut from, mut to) = (Kiocb::at(0), Kiocb::at(0));
    loop {
        let read = source.read_iter(&mut from, &mut buf)?;
        if read == 0 {
            return Ok(());
        }
        let mut written = 0;
        while written < read {
            match target.write_iter(&mut to, &buf[written..read])? {
                0 => {
                    return Err(FilesystemError::Other(
                        "a copy up's write wrote nothing".to_string(),
                    ))
                }
                wrote => written += wrote,
            }
        }
    }
}

/// The Layer's directory a level holds, which it holds wherever it shows
/// a name.
fn layer_dir(dir: &Option<OverlayPath>) -> Result<&OverlayPath, FilesystemError> {
    dir.as_ref().ok_or(FilesystemError::NotFound)
}

/// The directory `name` in `root`, made there first if it stands nowhere, as
/// overlayfs's `ovl_workdir_create` makes its upperdir's sibling.
fn ovl_workdir_create(root: &OverlayPath, name: &str) -> Result<OverlayPath, FilesystemError> {
    match root.do_mkdir(name) {
        Ok(()) | Err(FilesystemError::AlreadyExists) => root.open_dir(name),
        Err(err) => Err(err),
    }
}

/// Take `name` out of `dir`, a directory only once it is empty, as
/// overlayfs's `ovl_cleanup` takes away what a change left in its
/// workdir, so whatever another writer made in it meanwhile stays.
fn ovl_cleanup(dir: &OverlayPath, name: &str) -> Result<(), FilesystemError> {
    match dir.getattr_at(name)?.file_type {
        FileType::Dir => dir.do_rmdir(name),
        _ => dir.do_unlink(name),
    }
}

/// Take the whiteouts out of the directory `name` in `dir`, and nothing
/// else, as overlayfs's `ovl_cleanup_whiteouts` does before it removes
/// one; anything else stands there leaves it holding that.
fn ovl_cleanup_whiteouts(dir: &OverlayPath, name: &str) -> Result<(), FilesystemError> {
    if dir.getattr_at(name)?.file_type != FileType::Dir {
        return Ok(());
    }
    let inner = dir.open_dir(name)?;
    for entry in inner.entries()? {
        if inner.is_whiteout(&entry.name) {
            inner.do_unlink(&entry.name)?;
        }
    }
    Ok(())
}

/// Take `name` out of the workdir with all it holds, as overlayfs's
/// `ovl_workdir_cleanup` empties it of what an earlier mount left.
fn ovl_workdir_cleanup(dir: &OverlayPath, name: &str) -> Result<(), FilesystemError> {
    let mut pending = vec![(dir.clone(), name.to_string())];
    while let Some((dir, name)) = pending.last().cloned() {
        if dir.getattr_at(&name)?.file_type != FileType::Dir {
            dir.do_unlink(&name)?;
            pending.pop();
            continue;
        }
        let inner = dir.open_dir(&name)?;
        let entries = inner.entries()?;
        if entries.is_empty() {
            dir.do_rmdir(&name)?;
            pending.pop();
        }
        pending.extend(entries.into_iter().map(|entry| (inner.clone(), entry.name)));
    }
    Ok(())
}

/// Layers stacked into one view, topmost first, the upper among them on
/// top, as overlayfs's `ovl_fs` holds a mount's layers.
#[derive(Clone)]
pub(super) struct OverlayFs {
    /// Each Layer's root, the upper's `upper` at `UPPER` where there is one, and
    /// the lower Layers beneath it, topmost first.
    roots: Level,
    /// Where a change is built before it lands, with an upper only.
    work: Option<OverlayPath>,
    /// The next name a change built in `work` takes.
    temp: Arc<AtomicU64>,
}

impl OverlayFs {
    /// The view of `lower` under `upper`, its `upper` and `work` made where
    /// they stand nowhere and `work` emptied of what a change left, as
    /// overlayfs's mount does with its workdir.
    pub(super) fn new(
        lower: Vec<Arc<Filesystem>>,
        upper: Option<Arc<Filesystem>>,
    ) -> Result<Self, FilesystemError> {
        let mut roots = vec![None];
        let mut work = None;
        let creator_cred = Arc::default();
        if let Some(upper) = upper {
            let root = OverlayPath::root(&upper, &creator_cred);
            roots[UPPER] = Some(ovl_workdir_create(&root, UPPERDIR)?);
            let made = ovl_workdir_create(&root, WORKDIR)?;
            for entry in made.entries()? {
                ovl_workdir_cleanup(&made, &entry.name)?;
            }
            work = Some(made);
        }
        roots.extend(
            lower
                .iter()
                .map(|layer| Some(OverlayPath::root(layer, &creator_cred))),
        );
        Ok(Self {
            roots,
            work,
            temp: Arc::default(),
        })
    }

    /// Whether changes have an upper to land in, as `ovl_upper_mnt` answers.
    pub(super) fn has_upper(&self) -> bool {
        self.work.is_some()
    }

    /// What the topmost Layer's root recorded it stands for.
    pub(super) fn root_identity(&self) -> Option<Identity> {
        let topmost = self.roots.iter().flatten().next()?;
        topmost.dentry().inode()?.identity()
    }

    /// The state of the root, which stands for every Layer's root, as overlayfs's
    /// `ovl_get_root` keeps it on the root's inode as it mounts.
    pub(super) fn root_dir(&self) -> OverlayDirInode {
        OverlayDirInode::new(self.roots.clone())
    }

    /// The directory of the view `dentry` names, its state found on the inode
    /// its lookup, or the mount for the root, recorded.
    pub(super) fn dir(&self, dentry: &Dentry) -> Result<ViewDir, FilesystemError> {
        let inode = dentry.inode().ok_or(FilesystemError::NotFound)?;
        let inode = OverlayDirInode::of(&inode).ok_or(FilesystemError::NotADirectory)?;
        Ok(ViewDir {
            dentry: dentry.get(),
            inode,
        })
    }

    /// The directory holding the entry `dentry` names.
    pub(super) fn parent(&self, dentry: &Dentry) -> Result<ViewDir, FilesystemError> {
        let parent = dentry
            .parent()
            .expect("an entry the runtime names stands in a directory");
        self.dir(&parent)
    }

    /// The workdir and a name in it no change has taken, as overlayfs's
    /// `ovl_tempname` names one.
    fn tempname(&self) -> Result<(&OverlayPath, String), FilesystemError> {
        let work = self
            .work
            .as_ref()
            .ok_or(FilesystemError::ReadOnlyFilesystem)?;
        let next = self.temp.fetch_add(1, Ordering::Relaxed);
        Ok((work, format!("#{next:x}")))
    }
}
