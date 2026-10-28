//! The Layers of an Overlay and the rules that make them one view: which
//! Layer shows a name, how a change lands in the upper, and the markers
//! that hide what a lower Layer still holds. The calls on the
//! view are split as overlayfs splits them: finding a name in `namei`,
//! copying up in `copy_up`, listing in `readdir`, and changing a directory
//! in `dir`.
//!
//! A path is walked one name at a time from each Layer's root, as
//! overlayfs looks a name up in each Layer's directory, following no link
//! a Layer holds unless the caller asks, so every Layer is asked about a
//! single name in a directory already held. A Layer's directory merges
//! into the view only beneath a directory it also holds.
//!
//! Each change that touches more than one entry writes its marker or its
//! replacement before it takes the old one away, as overlayfs builds a
//! copy before it swaps it in, so a change that fails part way leaves the
//! view as it was or as it was meant to become.

mod copy_up;
mod dir;
mod namei;
mod readdir;

use std::sync::Arc;

use super::file::Readers;
use super::inode::Inodes;
use super::path::OverlayPath;

use super::super::path::join;
use super::super::{File, FilesystemError, Metadata, OpenOptions};
use crate::vfs::Filesystem;

/// The prefix of a whiteout, and of every name reserved for a marker.
const WHITEOUT: &str = ".wh.";
/// The marker hiding every lower Layer's entries of its directory.
const OPAQUE: &str = ".wh..wh..opq";

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

/// What `dir` holds at `name`, a link reported as itself.
fn entry(dir: &OverlayPath, name: &str) -> Result<Option<Metadata>, FilesystemError> {
    match dir.symlink_metadata(name) {
        Ok(metadata) => Ok(Some(metadata)),
        Err(FilesystemError::NotFound | FilesystemError::NotADirectory) => Ok(None),
        Err(err) => Err(err),
    }
}

/// Whether `dir` has an entry at `name`, a link pointing at nothing among
/// them. A Layer that cannot answer fails the call, so a marker it could
/// not read never lets a lower entry show.
fn holds(dir: &OverlayPath, name: &str) -> Result<bool, FilesystemError> {
    Ok(entry(dir, name)?.is_some())
}

/// Whether `dir` hides `name` in every Layer below it with a whiteout. A
/// directory's opaque marker hides every name at once, so a level never
/// holds the Layers below one; each name need not ask again.
fn hides(dir: &OverlayPath, name: &str) -> Result<bool, FilesystemError> {
    holds(dir, &whiteout(name))
}

/// `level` without the Layers below the topmost directory marked opaque,
/// as overlayfs stops looking further down once it finds one.
fn below_opaque_cut(mut level: Level) -> Result<Level, FilesystemError> {
    let mut cut = false;
    for index in 0..level.len() {
        if cut {
            level[index] = None;
        } else if let Some(shown) = &level[index] {
            cut = shows_below(&level, index) && holds(shown, OPAQUE)?;
        }
    }
    Ok(level)
}

/// Whether the directory `name` in `dir` holds an opaque marker.
fn opaque_inside(dir: &OverlayPath, name: &str) -> Result<bool, FilesystemError> {
    match dir.open_dir(name) {
        Ok(inner) => holds(&inner, OPAQUE),
        Err(
            FilesystemError::NotFound
            | FilesystemError::NotADirectory
            | FilesystemError::FilesystemLoop,
        ) => Ok(false),
        Err(err) => Err(err),
    }
}

/// Whether a Layer below `index` shows a directory in `level`, without
/// which a marker at `index` hides nothing, as overlayfs reads no opaque
/// marker in its last layer.
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

/// The whiteout hiding `name`, beside it in its directory.
fn whiteout(name: &str) -> String {
    format!("{WHITEOUT}{name}")
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

/// Take every marker out of `dir`, the upper's copy of a directory
/// the view shows empty, for `act` to change it; any other entry there
/// means it is not. When `act` fails, or a marker will not go, the markers
/// taken out are put back, so the view stays as it was.
fn without_markers(
    dir: &OverlayPath,
    act: impl FnOnce() -> Result<(), FilesystemError>,
) -> Result<(), FilesystemError> {
    let markers: Vec<String> = dir.entries()?.into_iter().map(|entry| entry.name).collect();
    if markers.iter().any(|name| !name.starts_with(WHITEOUT)) {
        return Err(FilesystemError::DirectoryNotEmpty);
    }
    let mut cleared = Vec::new();
    let changed = markers
        .iter()
        .try_for_each(|name| {
            dir.remove_file(name)?;
            cleared.push(name);
            Ok(())
        })
        .and_then(|()| act());
    if changed.is_err() {
        for marker in cleared {
            let _ = put_marker(dir, marker);
        }
    }
    changed
}

/// An empty file `name` in `dir`, as every marker is.
fn put_marker(dir: &OverlayPath, name: &str) -> Result<(), FilesystemError> {
    dir.open(
        name,
        &OpenOptions {
            write: true,
            create: true,
            ..OpenOptions::default()
        },
    )?
    .close()
}

/// The Layer's directory a level holds, which it holds wherever it shows
/// a name.
fn layer_dir(dir: &Option<OverlayPath>) -> Result<&OverlayPath, FilesystemError> {
    dir.as_ref().ok_or(FilesystemError::NotFound)
}

/// Refuse a name reserved for the Overlay's markers, in any case, since a
/// Layer that folds case would hold `.WH.name` as one.
pub(super) fn refuse_reserved(name: &str) -> Result<(), FilesystemError> {
    let reserved = name
        .get(..WHITEOUT.len())
        .is_some_and(|start| start.eq_ignore_ascii_case(WHITEOUT));
    if reserved {
        return Err(FilesystemError::InvalidInput);
    }
    Ok(())
}

/// Layers stacked into one view, topmost first, the upper among them on
/// top, as overlayfs's `ovl_fs` holds a mount's layers.
#[derive(Clone)]
pub(super) struct OverlayFs {
    layers: Vec<Arc<Filesystem>>,
    /// Whether the first Layer is the upper, where every change lands.
    has_upper: bool,
    readers: Arc<Readers>,
    inodes: Arc<Inodes>,
}

impl OverlayFs {
    pub(super) fn new(lower: Vec<Arc<Filesystem>>, upper: Option<Arc<Filesystem>>) -> Self {
        Self {
            has_upper: upper.is_some(),
            layers: upper.into_iter().chain(lower).collect(),
            readers: Arc::default(),
            inodes: Arc::default(),
        }
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
        self.has_upper && index == 0
    }

    /// The chain at the root of the view, every Layer's root merged down to
    /// the first marked opaque.
    pub(super) fn root(&self) -> Result<Chain, FilesystemError> {
        Ok(Chain {
            steps: vec![Step {
                name: String::new(),
                level: below_opaque_cut(
                    self.layers
                        .iter()
                        .map(|layer| Some(OverlayPath::root(layer)))
                        .collect(),
                )?,
            }],
        })
    }

    /// The upper, where every change lands; without one every change is
    /// refused as read-only.
    fn upper(&self) -> Result<usize, FilesystemError> {
        match self.has_upper {
            true => Ok(0),
            false => Err(FilesystemError::ReadOnlyFilesystem),
        }
    }
}
