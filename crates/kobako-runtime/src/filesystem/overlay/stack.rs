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

use super::file::Readers;
use super::inode::Inodes;
use super::path::OverlayPath;

use super::super::path::join;
use super::super::{File, FileType, FilesystemError, Metadata};
use crate::vfs::Filesystem;

/// The directories of the upper Tmpfs holding the view's changes and the
/// changes being built, as overlayfs's `upperdir` and `workdir`.
const UPPERDIR: &str = "upper";
const WORKDIR: &str = "work";

/// Each Layer's directory at one directory of the view, topmost first;
/// `None` where that Layer holds no directory the view merges there.
pub(super) type Level = Vec<Option<OverlayPath>>;

/// The directories of the view from the root to one of them.
#[derive(Clone)]
pub(super) struct Chain {
    steps: Vec<Step>,
}

#[derive(Clone)]
struct Step {
    name: String,
    level: Level,
}

impl Chain {
    /// A chain starting at the directory at `path` beneath the root, which
    /// `level` stands for.
    pub(super) fn at(path: String, level: Level) -> Self {
        Self {
            steps: vec![Step { name: path, level }],
        }
    }

    /// The directory the chain reaches.
    pub(super) fn level(&self) -> &Level {
        &self.last().level
    }

    /// Where the directory at `step` stands beneath the root.
    fn path_to(&self, step: usize) -> String {
        self.steps[..=step]
            .iter()
            .map(|step| step.name.as_str())
            .filter(|name| !name.is_empty())
            .collect::<Vec<_>>()
            .join("/")
    }

    fn last(&self) -> &Step {
        self.steps.last().expect("a chain holds the root")
    }

    /// Where the directory the chain reaches stands beneath the root.
    pub(super) fn path(&self) -> String {
        self.path_to(self.steps.len() - 1)
    }

    /// The path of `name` in the directory the chain reaches.
    pub(super) fn path_of(&self, name: &str) -> String {
        join(&self.path(), name)
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

fn copy_whole(source: &mut dyn File, target: &mut dyn File) -> Result<(), FilesystemError> {
    let mut buf = vec![0; 64 * 1024];
    let mut offset = 0;
    loop {
        let read = source.read_at(&mut buf, offset)?;
        if read == 0 {
            return Ok(());
        }
        let mut written = 0;
        while written < read {
            match target.write_at(&buf[written..read], offset + written as u64)? {
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

/// Take `name` out of `dir` with all it holds, as overlayfs's
/// `ovl_cleanup` takes away what a change left in its workdir.
fn cleanup(dir: &OverlayPath, name: &str) -> Result<(), FilesystemError> {
    if dir.symlink_metadata(name)?.file_type != FileType::Directory {
        return dir.remove_file(name);
    }
    let inner = dir.open_dir(name)?;
    for entry in inner.entries()? {
        cleanup(&inner, &entry.name)?;
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
    readers: Arc<Readers>,
    inodes: Arc<Inodes>,
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
                cleanup(&made, &entry.name)?;
            }
            work = Some(made);
        }
        roots.extend(lower.iter().map(OverlayPath::root));
        Ok(Self {
            roots,
            work,
            temp: Arc::default(),
            readers: Arc::default(),
            inodes: Arc::default(),
        })
    }

    pub(super) fn inodes(&self) -> &Inodes {
        &self.inodes
    }

    /// Whether the upper, where every copy up lands, holds no directory in
    /// `level`, so a change through it may have to make one there first.
    /// Only such a directory never moves through the Overlay.
    pub(super) fn misses_upper(&self, level: &Level) -> bool {
        self.upper().is_ok_and(|upper| level[upper].is_none())
    }

    fn is_upper(&self, index: usize) -> bool {
        self.work.is_some() && index == 0
    }

    /// The chain at the root of the view, every Layer's root merged down to
    /// the first marked opaque.
    pub(super) fn root(&self) -> Result<Chain, FilesystemError> {
        Ok(Chain {
            steps: vec![Step {
                name: String::new(),
                level: below_opaque_cut(self.roots.iter().cloned().map(Some).collect()),
            }],
        })
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
