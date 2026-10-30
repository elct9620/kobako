//! The Layers of an Overlay and the rules that make them one view: which
//! Layer shows a name, how a change lands in the upper, and the marks that
//! hide what a lower Layer still holds. The calls on the view are split
//! as overlayfs splits them: finding a name in `namei`, copying up in
//! `copy_up`, listing in `readdir`, and changing a directory in `dir`.
//!
//! A path is walked one name at a time from each Layer's root, as
//! overlayfs looks a name up in each Layer's directory, following no link
//! a Layer holds unless the caller asks, so every Layer is asked about a
//! single name in a directory already held. A Layer's directory merges
//! into the view only beneath a directory it also holds.
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

use super::inode::{ovl_inode, OverlayInode};
use super::path::OverlayPath;

use super::super::{FileOperations, FileType, FilesystemError, Metadata};
use crate::vfs::{Blocking, Dentry, Filesystem, Inline};

/// The directories of the upper Tmpfs holding the view's changes and the
/// changes being built, as overlayfs's `upperdir` and `workdir`.
const UPPERDIR: &str = "upper";
const WORKDIR: &str = "work";

/// Each Layer's directory at one directory of the view, topmost first;
/// `None` where that Layer holds no directory the view merges there.
pub(super) type Level = Vec<Option<OverlayPath>>;

/// One directory of the view: the dentry the runtime holds for it and the
/// state kept there, as overlayfs reaches a directory by its dentry and
/// the `ovl_inode` behind it.
#[derive(Clone)]
pub(super) struct ViewDir {
    dentry: Arc<Dentry>,
    inode: Arc<OverlayInode>,
}

impl ViewDir {
    /// Each Layer's directory this one stands for.
    pub(super) fn level(&self) -> Level {
        self.inode.level()
    }
}

/// What `dir` holds at `name`, a link reported as itself and a whiteout
/// as nothing.
fn entry(dir: &OverlayPath, name: &str) -> Result<Option<Metadata>, FilesystemError> {
    if dir.is_whiteout(name) {
        return Ok(None);
    }
    match dir.symlink_metadata(name) {
        Ok(metadata) => Ok(Some(metadata)),
        Err(FilesystemError::NotFound | FilesystemError::NotADirectory) => Ok(None),
        Err(err) => Err(err),
    }
}

/// Whether `dir` has an entry at `name`, a link pointing at nothing among
/// them.
fn holds(dir: &OverlayPath, name: &str) -> Result<bool, FilesystemError> {
    Ok(entry(dir, name)?.is_some())
}

/// `level` without the Layers below the topmost directory marked opaque,
/// as overlayfs stops looking further down once it finds one.
fn below_opaque_cut(mut level: Level) -> Level {
    let mut cut = false;
    for index in 0..level.len() {
        if cut {
            level[index] = None;
        } else if let Some(shown) = &level[index] {
            cut = shows_below(&level, index) && shown.is_opaque();
        }
    }
    level
}

/// Whether the directory `name` in `dir` is marked opaque.
fn opaque_inside(dir: &OverlayPath, name: &str) -> Result<bool, FilesystemError> {
    match dir.open_dir(name) {
        Ok(inner) => Ok(inner.is_opaque()),
        Err(
            FilesystemError::NotFound
            | FilesystemError::NotADirectory
            | FilesystemError::FilesystemLoop,
        ) => Ok(false),
        Err(err) => Err(err),
    }
}

/// Whether a Layer below `index` shows a directory in `level`, without
/// which a mark at `index` hides nothing, as overlayfs reads no opaque
/// mark in its last layer.
fn shows_below(level: &Level, index: usize) -> bool {
    level[index + 1..].iter().any(Option::is_some)
}

/// The one Layer showing the directory `level`, if no other does. Nothing
/// there merges or hides, so the Layer's own directory answers for each
/// name beneath, opened as that Layer opens one, as overlayfs leaves a
/// directory only one layer holds unmerged.
fn sole_layer(level: &Level) -> Option<(usize, &OverlayPath)> {
    let mut shown = level
        .iter()
        .enumerate()
        .filter_map(|(index, dir)| Some((index, dir.as_ref()?)));
    match (shown.next(), shown.next()) {
        (Some(only), None) => Some(only),
        _ => None,
    }
}

fn copy_whole(
    source: &mut dyn FileOperations,
    target: &mut dyn FileOperations,
) -> Result<(), FilesystemError> {
    let mut buf = vec![0; 64 * 1024];
    let mut offset = 0;
    loop {
        let read = source.read(&mut buf, offset)?;
        if read == 0 {
            return Ok(());
        }
        let mut written = 0;
        while written < read {
            match target.write(&buf[written..read], offset + written as u64)? {
                0 => {
                    return Err(FilesystemError::Other(
                        "a copy up's write wrote nothing".to_string(),
                    ))
                }
                wrote => written += wrote,
            }
        }
        offset += read as u64;
    }
}

/// The Layer's directory a level holds, which it holds wherever it shows
/// a name.
fn layer_dir(dir: &Option<OverlayPath>) -> Result<&OverlayPath, FilesystemError> {
    dir.as_ref().ok_or(FilesystemError::NotFound)
}

/// The directory `name` in `root`, made there first if it stands nowhere.
fn subdir(root: &OverlayPath, name: &str) -> Result<OverlayPath, FilesystemError> {
    match root.create_dir(name) {
        Ok(()) | Err(FilesystemError::AlreadyExists) => root.open_dir(name),
        Err(err) => Err(err),
    }
}

/// Take `name` out of `dir`, a directory only once it is empty, as
/// overlayfs's `ovl_cleanup` takes away what a change left in its
/// workdir, so whatever another writer made in it meanwhile stays.
fn cleanup(dir: &OverlayPath, name: &str) -> Result<(), FilesystemError> {
    match dir.symlink_metadata(name)?.file_type {
        FileType::Directory => dir.remove_dir(name),
        _ => dir.remove_file(name),
    }
}

/// Take the whiteouts out of the directory `name` in `dir`, and nothing
/// else, as overlayfs's `ovl_cleanup_whiteouts` does before it removes
/// one; anything else stands there leaves it holding that.
fn cleanup_whiteouts(dir: &OverlayPath, name: &str) -> Result<(), FilesystemError> {
    let inner = match dir.open_dir(name) {
        Ok(inner) => inner,
        Err(FilesystemError::NotADirectory) => return Ok(()),
        Err(err) => return Err(err),
    };
    for entry in inner.entries()? {
        if inner.is_whiteout(&entry.name) {
            inner.remove_file(&entry.name)?;
        }
    }
    Ok(())
}

/// Take `name` out of the workdir with all it holds, as overlayfs's
/// `ovl_workdir_cleanup` empties it of what an earlier mount left.
fn workdir_cleanup(dir: &OverlayPath, name: &str) -> Result<(), FilesystemError> {
    if dir.symlink_metadata(name)?.file_type != FileType::Directory {
        return dir.remove_file(name);
    }
    let inner = dir.open_dir(name)?;
    for entry in inner.entries()? {
        workdir_cleanup(&inner, &entry.name)?;
    }
    dir.remove_dir(name)
}

/// Layers stacked into one view, topmost first, the upper among them on
/// top, as overlayfs's `ovl_fs` holds a mount's layers.
#[derive(Clone)]
pub(super) struct OverlayFs {
    /// Each Layer's root, topmost first; the upper's is its `upper`.
    roots: Vec<OverlayPath>,
    /// Where a change is built before it lands, with an upper only.
    work: Option<OverlayPath>,
    /// The next name a change built in `work` takes.
    temp: Arc<AtomicU64>,
    /// What a copy up waiting for another's lock gives up meanwhile.
    blocking: Arc<dyn Blocking>,
}

impl OverlayFs {
    /// The view of `lower` under `upper`, its `upper` and `work` made where
    /// they stand nowhere and `work` emptied of what a change left, as
    /// overlayfs's mount does with its workdir.
    pub(super) fn new(
        lower: Vec<Arc<Filesystem>>,
        upper: Option<Arc<Filesystem>>,
    ) -> Result<Self, FilesystemError> {
        let mut roots = Vec::new();
        let mut work = None;
        if let Some(upper) = upper {
            let root = OverlayPath::root(&upper);
            roots.push(subdir(&root, UPPERDIR)?);
            let made = subdir(&root, WORKDIR)?;
            for entry in made.entries()? {
                workdir_cleanup(&made, &entry.name)?;
            }
            work = Some(made);
        }
        roots.extend(lower.iter().map(OverlayPath::root));
        Ok(Self {
            roots,
            work,
            temp: Arc::default(),
            blocking: Arc::new(Inline),
        })
    }

    /// This view waiting for a copy up's lock through `blocking`.
    pub(super) fn with_blocking(self, blocking: Arc<dyn Blocking>) -> Self {
        Self { blocking, ..self }
    }

    /// Whether `other` is this view, reached through another directory.
    pub(super) fn is(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.temp, &other.temp)
    }

    fn is_upper(&self, index: usize) -> bool {
        self.work.is_some() && index == 0
    }

    /// The directory of the view `dentry` names, its state found on the
    /// dentry or, the first time it is reached, looked up beneath its
    /// parent: the root's every Layer's root merged down to the first
    /// marked opaque.
    pub(super) fn dir(&self, dentry: &Dentry) -> Result<ViewDir, FilesystemError> {
        let inode = ovl_inode(dentry, || match dentry.dget_parent() {
            None => Ok(below_opaque_cut(
                self.roots.iter().cloned().map(Some).collect(),
            )),
            Some(parent) => self.enter(&self.dir(&parent)?.level(), &dentry.name()),
        })?;
        Ok(ViewDir {
            dentry: dentry.dget(),
            inode,
        })
    }

    /// The directory holding the entry `dentry` names.
    pub(super) fn parent(&self, dentry: &Dentry) -> Result<ViewDir, FilesystemError> {
        let parent = dentry
            .dget_parent()
            .expect("an entry the runtime names stands in a directory");
        self.dir(&parent)
    }

    /// The upper, where every change lands; without one every change is
    /// refused as read-only.
    fn upper(&self) -> Result<usize, FilesystemError> {
        match self.work {
            Some(_) => Ok(0),
            None => Err(FilesystemError::ReadOnlyFilesystem),
        }
    }

    /// The workdir and a name in it no change has taken, as overlayfs's
    /// `ovl_tempname` names one.
    fn temp(&self) -> Result<(&OverlayPath, String), FilesystemError> {
        let work = self
            .work
            .as_ref()
            .ok_or(FilesystemError::ReadOnlyFilesystem)?;
        let next = self.temp.fetch_add(1, Ordering::Relaxed);
        Ok((work, format!("#{next:x}")))
    }
}
