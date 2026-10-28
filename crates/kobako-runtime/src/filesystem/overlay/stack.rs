//! The Layers of an Overlay and the rules that make them one view: which
//! Layer shows a name, how a change lands in a writable Layer, and the
//! markers that hide what a read-only Layer still holds.
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

use std::collections::HashSet;
use std::sync::Arc;

use super::lower::Lower;
use super::merged::MergedDirs;
use super::readers::Readers;

use super::super::path::{beneath_itself, is_beneath, join};
use super::super::{
    Access, DirEntry, File, FileType, FilesystemError, Metadata, OpenOptions, Rename,
};
use crate::vfs::Filesystem;

/// The prefix of a whiteout, and of every name reserved for a marker.
const WHITEOUT: &str = ".wh.";
/// The marker hiding every lower Layer's entries of its directory.
const OPAQUE: &str = ".wh..wh..opq";

#[derive(Clone)]
struct Layer {
    filesystem: Arc<Filesystem>,
    access: Access,
}

impl Layer {
    fn writable(&self) -> bool {
        self.access == Access::ReadWrite
    }
}

/// Each Layer's directory at one directory of the view, topmost first;
/// `None` where that Layer holds no directory the view merges there.
pub(super) type Level = Vec<Option<Lower>>;

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
fn entry(dir: &Lower, name: &str) -> Result<Option<Metadata>, FilesystemError> {
    match dir.symlink_metadata(name) {
        Ok(metadata) => Ok(Some(metadata)),
        Err(FilesystemError::NotFound | FilesystemError::NotADirectory) => Ok(None),
        Err(err) => Err(err),
    }
}

/// Whether `dir` has an entry at `name`, a link pointing at nothing among
/// them. A Layer that cannot answer fails the call, so a marker it could
/// not read never lets a lower entry show.
fn holds(dir: &Lower, name: &str) -> Result<bool, FilesystemError> {
    Ok(entry(dir, name)?.is_some())
}

/// Whether `dir` hides `name` in every Layer below it with a whiteout. A
/// directory's opaque marker hides every name at once, so a level never
/// holds the Layers below one; each name need not ask again.
fn hides(dir: &Lower, name: &str) -> Result<bool, FilesystemError> {
    holds(dir, &whiteout(name))
}

/// `level` without the Layers below the topmost directory marked opaque,
/// as overlayfs stops looking further down once it finds one.
fn below_opaque_cut(mut level: Level) -> Result<Level, FilesystemError> {
    let mut cut = false;
    for dir in &mut level {
        if cut {
            *dir = None;
        } else if let Some(shown) = dir {
            cut = holds(shown, OPAQUE)?;
        }
    }
    Ok(level)
}

/// Whether the directory `name` in `dir` holds an opaque marker.
fn opaque_inside(dir: &Lower, name: &str) -> Result<bool, FilesystemError> {
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

/// Take every marker out of `dir`, a writable Layer's copy of a directory
/// the view shows empty, for `act` to change it; any other entry there
/// means it is not. When `act` fails, or a marker will not go, the markers
/// taken out are put back, so the view stays as it was.
fn without_markers(
    dir: &Lower,
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
fn put_marker(dir: &Lower, name: &str) -> Result<(), FilesystemError> {
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

/// Layers stacked into one view, the first added on top.
#[derive(Clone, Default)]
pub(super) struct Stack {
    layers: Vec<Layer>,
    readers: Arc<Readers>,
    merged_dirs: Arc<MergedDirs>,
}

impl Stack {
    pub(super) fn merged_dirs(&self) -> &MergedDirs {
        &self.merged_dirs
    }

    /// Whether the topmost writable Layer, where every copy up lands, holds
    /// no directory in `level`, so a change through it may have to make one
    /// there first. Only such a directory never moves through the Overlay.
    pub(super) fn misses_writable(&self, level: &Level) -> bool {
        self.upper().is_ok_and(|upper| level[upper].is_none())
    }

    pub(super) fn push(&mut self, filesystem: Arc<Filesystem>, access: Access) {
        self.layers.push(Layer { filesystem, access });
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
                        .map(|layer| Some(Lower::root(&layer.filesystem)))
                        .collect(),
                )?,
            }],
        })
    }

    /// Walk `chain` into each of `names` in turn, each a directory the view
    /// shows.
    pub(super) fn descend<'a>(
        &self,
        chain: &mut Chain,
        names: impl IntoIterator<Item = &'a str>,
    ) -> Result<(), FilesystemError> {
        for name in names {
            self.enter(chain, name)?;
        }
        Ok(())
    }

    /// Step `chain` into the directory `name`, refusing a link standing
    /// there as a loop, since the runtime follows a link itself, in the
    /// view.
    pub(super) fn enter(&self, chain: &mut Chain, name: &str) -> Result<(), FilesystemError> {
        refuse_reserved(name)?;
        let level = chain.level();
        let (index, found) = self.lookup(level, name)?.ok_or(FilesystemError::NotFound)?;
        let step = match found.file_type {
            FileType::Directory => Step {
                name: name.to_string(),
                level: self.merged(level, name, (index, found))?,
            },
            FileType::Symlink => return Err(FilesystemError::FilesystemLoop),
            _ => return Err(FilesystemError::NotADirectory),
        };
        chain.steps.push(step);
        Ok(())
    }

    /// The topmost Layer holding an entry at `name` in the directory
    /// `level`, and that entry, a link reported as itself.
    fn lookup(
        &self,
        level: &Level,
        name: &str,
    ) -> Result<Option<(usize, Metadata)>, FilesystemError> {
        for (index, dir) in level.iter().enumerate() {
            let Some(dir) = dir else {
                continue;
            };
            if let Some(found) = entry(dir, name)? {
                return Ok(Some((index, found)));
            }
            if hides(dir, name)? {
                break;
            }
        }
        Ok(None)
    }

    /// The directory `name` in `level`, found as `found` names it first: each
    /// Layer's own directory there, merged downward until a Layer holds
    /// something else there, hides it, or marks its directory opaque, as
    /// overlayfs merges only directories.
    fn merged(
        &self,
        level: &Level,
        name: &str,
        found: (usize, Metadata),
    ) -> Result<Level, FilesystemError> {
        let mut merged = vec![None; level.len()];
        for (index, dir) in level.iter().enumerate().skip(found.0) {
            let Some(dir) = dir else {
                continue;
            };
            let standing = if index == found.0 {
                Some(found.1)
            } else {
                entry(dir, name)?
            };
            match standing {
                Some(metadata) if metadata.file_type == FileType::Directory => {
                    let inner = dir.open_dir(name)?;
                    let opaque = holds(&inner, OPAQUE)?;
                    merged[index] = Some(inner);
                    if opaque {
                        break;
                    }
                }
                Some(_) => break,
                None => {}
            }
            if hides(dir, name)? {
                break;
            }
        }
        Ok(merged)
    }

    /// What the view reports for the directory `level`, as overlayfs
    /// reports a merged directory: the identity of the lowest Layer showing
    /// it, so copying it up into a writable Layer leaves the identity as it
    /// was, the times of the topmost, and one name where more than one
    /// Layer shows it.
    pub(super) fn dir_metadata(&self, level: &Level) -> Result<Metadata, FilesystemError> {
        let mut shown = level.iter().flatten();
        let topmost = shown.next().ok_or(FilesystemError::NotFound)?.metadata()?;
        let lowest = match shown.last() {
            Some(lowest) => lowest.metadata()?.with_nlink(1),
            None => topmost,
        };
        Ok(Metadata {
            accessed: topmost.accessed,
            modified: topmost.modified,
            changed: topmost.changed,
            ..lowest
        })
    }

    /// `found` at `name` in the Layer at `index`, as the view reports it: a
    /// directory takes the identity of the lowest Layer showing it.
    fn merge_identity(
        &self,
        level: &Level,
        name: &str,
        index: usize,
        found: Metadata,
    ) -> Result<Metadata, FilesystemError> {
        if found.file_type != FileType::Directory {
            return Ok(found);
        }
        self.dir_metadata(&self.merged(level, name, (index, found))?)
    }

    /// What the Layer showing `name` holds there, a link it holds
    /// reported as one even where it points at nothing.
    pub(super) fn symlink_metadata(
        &self,
        level: &Level,
        name: &str,
    ) -> Result<Metadata, FilesystemError> {
        refuse_reserved(name)?;
        let (index, found) = self.lookup(level, name)?.ok_or(FilesystemError::NotFound)?;
        self.merge_identity(level, name, index, found)
    }

    /// `name` spelled as the topmost Layer showing it stores it; a name no
    /// Layer shows keeps its spelling.
    pub(super) fn stored_name(&self, level: &Level, name: &str) -> Result<String, FilesystemError> {
        match self.lookup(level, name)? {
            Some((index, _)) => level[index]
                .as_ref()
                .ok_or(FilesystemError::NotFound)?
                .stored_name(name),
            None => Ok(name.to_string()),
        }
    }

    /// Where the link the view shows at `name` points, as the Layer
    /// holding it reads it.
    pub(super) fn read_link(&self, level: &Level, name: &str) -> Result<String, FilesystemError> {
        refuse_reserved(name)?;
        match self.lookup(level, name)?.ok_or(FilesystemError::NotFound)? {
            (index, found) if found.file_type == FileType::Symlink => {
                held(&level[index])?.read_link(name)
            }
            _ => Err(FilesystemError::InvalidInput),
        }
    }

    /// Every Layer still showing `name` in `level`, topmost first, and the
    /// topmost Layer below them whose entry there a file or link above
    /// shadows.
    fn holders(
        &self,
        level: &Level,
        name: &str,
    ) -> Result<(Vec<usize>, Option<usize>), FilesystemError> {
        let mut holders = Vec::new();
        for (index, dir) in level.iter().enumerate() {
            let Some(dir) = dir else {
                continue;
            };
            let found = entry(dir, name)?;
            let hidden = match &found {
                Some(metadata) => {
                    holders.push(index);
                    metadata.file_type != FileType::Directory
                        || hides(dir, name)?
                        || opaque_inside(dir, name)?
                }
                None => hides(dir, name)?,
            };
            if hidden {
                let shadowed = match found {
                    Some(_) => self.beneath(level, index, name)?,
                    None => None,
                };
                return Ok((holders, shadowed));
            }
        }
        Ok((holders, None))
    }

    /// The topmost Layer below `above` holding `name` in sight.
    fn beneath(
        &self,
        level: &Level,
        above: usize,
        name: &str,
    ) -> Result<Option<usize>, FilesystemError> {
        for (index, dir) in level.iter().enumerate().skip(above + 1) {
            let Some(dir) = dir else {
                continue;
            };
            if holds(dir, name)? {
                return Ok(Some(index));
            }
            if hides(dir, name)? {
                return Ok(None);
            }
        }
        Ok(None)
    }

    /// The topmost writable Layer, where every change that is not in place
    /// lands.
    fn upper(&self) -> Result<usize, FilesystemError> {
        self.layers
            .iter()
            .position(Layer::writable)
            .ok_or(FilesystemError::ReadOnlyFilesystem)
    }

    /// Refuse as read-only a change landing at `name` in Layer `into` that
    /// a read-only Layer above it would cover, holding or hiding `name` or
    /// marking its directory opaque, so no change succeeds out of sight.
    fn refuse_covered(
        &self,
        level: &Level,
        name: &str,
        into: usize,
    ) -> Result<(), FilesystemError> {
        for dir in level[..into].iter().flatten() {
            if holds(dir, name)? || hides(dir, name)? || holds(dir, OPAQUE)? {
                return Err(FilesystemError::ReadOnlyFilesystem);
            }
        }
        Ok(())
    }

    /// The directory `chain` reaches in Layer `into`, made there along
    /// with each directory above it that so far shows only from another
    /// Layer.
    fn upper_dir(&self, chain: &mut Chain, into: usize) -> Result<Lower, FilesystemError> {
        self.made(chain, chain.steps.len() - 1, into)
    }

    fn made(&self, chain: &mut Chain, step: usize, into: usize) -> Result<Lower, FilesystemError> {
        if let Some(dir) = &chain.steps[step].level[into] {
            return Ok(dir.clone());
        }
        if step == 0 {
            return Err(FilesystemError::CrossesDevices);
        }
        let parent = self.made(chain, step - 1, into)?;
        let name = chain.steps[step].name.clone();
        let dir = match entry(&parent, &name)? {
            Some(found) if found.file_type == FileType::Directory => parent.open_dir(&name)?,
            Some(found) if found.file_type == FileType::Symlink => {
                return Err(FilesystemError::CrossesDevices)
            }
            Some(_) => return Err(FilesystemError::NotADirectory),
            None => {
                parent.create_dir(&name)?;
                parent.open_dir(&name)?
            }
        };
        chain.steps[step].level[into] = Some(dir.clone());
        self.merged_dirs.copied_up(&chain.path_to(step), into, &dir);
        Ok(dir)
    }

    /// Copy the file `name` from Layer `from` into Layer `into` whole.
    /// Layer `into` held nothing there, so a copy that fails part way is
    /// taken away again rather than left to hide the whole file. A link
    /// is not copied up, as no Layer is handed one to hold.
    fn copy_up_file(
        &self,
        chain: &mut Chain,
        name: &str,
        from: usize,
        into: usize,
    ) -> Result<(), FilesystemError> {
        let source_dir = chain.level()[from]
            .clone()
            .ok_or(FilesystemError::NotFound)?;
        if entry(&source_dir, name)?.map(|found| found.file_type) == Some(FileType::Symlink) {
            return Err(FilesystemError::CrossesDevices);
        }
        let target_dir = self.upper_dir(chain, into)?;
        let mut source = source_dir.open(
            name,
            &OpenOptions {
                read: true,
                nofollow: true,
                ..OpenOptions::default()
            },
        )?;
        let mut copy = target_dir.open(
            name,
            &OpenOptions {
                write: true,
                create: true,
                truncate: true,
                ..OpenOptions::default()
            },
        )?;
        let copied = copy_whole(source.as_mut(), copy.as_mut()).and(copy.close());
        if copied.is_err() {
            let _ = target_dir.remove_file(name);
            return copied;
        }
        self.readers.copied(&chain.path_of(name), &target_dir, name)
    }

    /// Clear the whiteout of `name` in `dir` once an entry has landed
    /// there. A whiteout left beside the entry hides only what lies below,
    /// which the entry replaced, so failing to clear it fails nothing.
    fn unhide(dir: &Lower, name: &str) {
        let _ = dir.remove_file(&whiteout(name));
    }

    fn mark(&self, chain: &mut Chain, marker: &str, into: usize) -> Result<(), FilesystemError> {
        put_marker(&self.upper_dir(chain, into)?, marker)
    }

    /// The Layer a whiteout over the Layer at `shown` lands in, or `None`
    /// when nothing below needs one; refused as read-only when no writable
    /// Layer sits above it.
    fn whiteout_layer(&self, shown: Option<usize>) -> Result<Option<usize>, FilesystemError> {
        match shown {
            Some(shown) => match self.upper()? {
                upper if upper < shown => Ok(Some(upper)),
                _ => Err(FilesystemError::ReadOnlyFilesystem),
            },
            None => Ok(None),
        }
    }

    /// Take `name` out of the view: a whiteout over any Layer that keeps it
    /// first, then out of every other Layer holding it. A read-only Layer
    /// keeps it, and so does a writable one `keeps` answers for, so no
    /// Layer changes before each is known to go; `remove` takes it out of
    /// one Layer's directory.
    fn withdraw(
        &self,
        chain: &mut Chain,
        name: &str,
        keeps: impl Fn(&Lower) -> Result<bool, FilesystemError>,
        remove: impl Fn(&Lower) -> Result<(), FilesystemError>,
    ) -> Result<(), FilesystemError> {
        let level = chain.level().clone();
        let (holders, shadowed) = self.holders(&level, name)?;
        let mut kept = Vec::new();
        for &index in &holders {
            let dir = level[index].as_ref().ok_or(FilesystemError::NotFound)?;
            if !self.layers[index].writable() || keeps(dir)? {
                kept.push(index);
            }
        }
        let shown = kept.first().copied().into_iter().chain(shadowed).min();
        if let Some(upper) = self.whiteout_layer(shown)? {
            self.mark(chain, &whiteout(name), upper)?;
        }
        for &index in &holders {
            if !kept.contains(&index) {
                remove(level[index].as_ref().ok_or(FilesystemError::NotFound)?)?;
            }
        }
        Ok(())
    }

    pub(super) fn open(
        &self,
        chain: &mut Chain,
        name: &str,
        options: &OpenOptions,
    ) -> Result<Box<dyn File>, FilesystemError> {
        refuse_reserved(name)?;
        let changes = options.write || options.truncate;
        let level = chain.level().clone();
        match self.lookup(&level, name)? {
            Some((_, found)) if found.file_type == FileType::Directory => {
                Err(FilesystemError::IsADirectory)
            }
            Some(_) if options.create_new => Err(FilesystemError::AlreadyExists),
            Some((index, _)) if self.layers[index].writable() => {
                held(&level[index])?.open(name, options)
            }
            Some((index, _)) if !changes => {
                let file = held(&level[index])?.open(name, options)?;
                Ok(self.readers.reading(&chain.path_of(name), file))
            }
            Some((index, _)) => {
                let into = self.layers[..index]
                    .iter()
                    .rposition(Layer::writable)
                    .ok_or(FilesystemError::ReadOnlyFilesystem)?;
                self.copy_up_file(chain, name, index, into)?;
                held(&chain.level()[into])?.open(name, options)
            }
            None if options.create || options.create_new => self.create_file(chain, name, options),
            None => Err(FilesystemError::NotFound),
        }
    }

    /// A link pointing at nothing still names an entry, so an exclusive
    /// create finds it there; any other create reaches through it, which
    /// only the Layer holding the link may answer. The new file shadows
    /// whatever the whiteout hid before the whiteout goes, so nothing
    /// shows through in between.
    fn create_file(
        &self,
        chain: &mut Chain,
        name: &str,
        options: &OpenOptions,
    ) -> Result<Box<dyn File>, FilesystemError> {
        let level = chain.level().clone();
        match self.lookup(&level, name)? {
            Some(_) if options.create_new => Err(FilesystemError::AlreadyExists),
            Some((index, _)) if self.layers[index].writable() => {
                held(&level[index])?.open(name, options)
            }
            Some(_) => Err(FilesystemError::CrossesDevices),
            None => {
                let upper = self.upper()?;
                self.refuse_covered(&level, name, upper)?;
                let dir = self.upper_dir(chain, upper)?;
                let file = dir.open(name, options)?;
                Self::unhide(&dir, name);
                Ok(file)
            }
        }
    }

    pub(super) fn entries(&self, level: &Level) -> Result<Vec<DirEntry>, FilesystemError> {
        let mut entries: Vec<DirEntry> = Vec::new();
        let mut taken: HashSet<String> = HashSet::new();
        for dir in level.iter().flatten() {
            let listed = dir.entries()?;
            for entry in &listed {
                if !entry.name.starts_with(WHITEOUT) && taken.insert(entry.name.clone()) {
                    entries.push(entry.clone());
                }
            }
            taken.extend(
                listed
                    .iter()
                    .filter_map(|entry| entry.name.strip_prefix(WHITEOUT))
                    .map(str::to_string),
            );
            if listed.iter().any(|entry| entry.name == OPAQUE) {
                break;
            }
        }
        Ok(entries)
    }

    /// Whether the directory `name` in `level` shows no entry.
    fn is_empty(
        &self,
        level: &Level,
        name: &str,
        found: (usize, Metadata),
    ) -> Result<bool, FilesystemError> {
        Ok(self.entries(&self.merged(level, name, found)?)?.is_empty())
    }

    /// A directory made where a whiteout hid one is marked opaque before
    /// the whiteout goes, so the lower Layers' entries never show in it.
    pub(super) fn create_dir(&self, chain: &mut Chain, name: &str) -> Result<(), FilesystemError> {
        refuse_reserved(name)?;
        let level = chain.level().clone();
        if self.lookup(&level, name)?.is_some() {
            return Err(FilesystemError::AlreadyExists);
        }
        let upper = self.upper()?;
        self.refuse_covered(&level, name, upper)?;
        let dir = self.upper_dir(chain, upper)?;
        let hid = holds(&dir, &whiteout(name))?;
        dir.create_dir(name)?;
        if hid {
            let marked = dir
                .open_dir(name)
                .and_then(|made| put_marker(&made, OPAQUE));
            if let Err(err) = marked {
                let _ = dir.remove_dir(name);
                return Err(err);
            }
            Self::unhide(&dir, name);
        }
        Ok(())
    }

    /// A writable Layer's copy of the directory may hold only markers; any
    /// other entry there means the view is not what was found empty. A
    /// copy still holding what a Layer above hides stays, beneath the
    /// whiteout.
    pub(super) fn remove_dir(&self, chain: &mut Chain, name: &str) -> Result<(), FilesystemError> {
        refuse_reserved(name)?;
        let level = chain.level().clone();
        let (index, found) = self
            .lookup(&level, name)?
            .ok_or(FilesystemError::NotFound)?;
        if found.file_type != FileType::Directory {
            return Err(FilesystemError::NotADirectory);
        }
        if !self.is_empty(&level, name, (index, found))? {
            return Err(FilesystemError::DirectoryNotEmpty);
        }
        self.withdraw(
            chain,
            name,
            |dir| {
                Ok(dir
                    .open_dir(name)?
                    .entries()?
                    .iter()
                    .any(|entry| !entry.name.starts_with(WHITEOUT)))
            },
            |dir| {
                let inner = dir.open_dir(name)?;
                without_markers(&inner, || dir.remove_dir(name))
            },
        )
    }

    /// A link is removed as itself, wherever it points.
    pub(super) fn remove_file(&self, chain: &mut Chain, name: &str) -> Result<(), FilesystemError> {
        if self.symlink_metadata(chain.level(), name)?.file_type == FileType::Directory {
            return Err(FilesystemError::IsADirectory);
        }
        self.withdraw(chain, name, |_| Ok(false), |dir| dir.remove_file(name))
    }

    /// A directory moves only when the topmost writable Layer alone holds
    /// it; moving one out of another would mean copying up everything
    /// beneath it, so it is refused as crossing devices, as overlayfs
    /// refuses it without `redirect_dir`; so is a link or another kind a
    /// lower Layer holds, which no copy up can make. What the view holds
    /// at `to` is checked as rename(2) checks it, and a directory moved
    /// over one a lower Layer holds is marked opaque before it moves, so
    /// it never lands without the marker. The source is whited out before
    /// it moves, so it never shows from a read-only Layer once moved. What
    /// a lower writable Layer holds is refused as crossing devices, as
    /// unionfs-fuse refuses a rename across writable branches, since moving
    /// it would change two Layers that could fail apart.
    pub(super) fn rename(
        &self,
        from: &mut Chain,
        from_name: &str,
        to: &mut Chain,
        to_name: &str,
        rename: Rename,
    ) -> Result<(), FilesystemError> {
        refuse_reserved(to_name)?;
        let from_level = from.level().clone();
        let moving = self.symlink_metadata(&from_level, from_name)?;
        let (from_path, to_path) = (from.path_of(from_name), to.path_of(to_name));
        if from_path == to_path {
            return Ok(());
        }
        rename.check(moving.file_type)?;
        if is_beneath(&to_path, &from_path) {
            return Err(beneath_itself(moving.file_type));
        }
        let to_level = to.level().clone();
        let replaced = self.lookup(&to_level, to_name)?;
        // Only a directory replaces a directory, and only an empty one.
        match (moving.file_type == FileType::Directory, replaced) {
            (false, Some((_, found))) if found.file_type == FileType::Directory => {
                return Err(FilesystemError::IsADirectory)
            }
            (true, Some((index, found)))
                if found.file_type == FileType::Directory
                    && !self.is_empty(&to_level, to_name, (index, found))? =>
            {
                return Err(FilesystemError::DirectoryNotEmpty)
            }
            (true, Some((_, found))) if found.file_type != FileType::Directory => {
                return Err(FilesystemError::NotADirectory)
            }
            _ => {}
        }
        let upper = self.upper()?;
        self.refuse_covered(&to_level, to_name, upper)?;
        let mut replaces_below = false;
        if moving.file_type == FileType::Directory {
            for (index, dir) in to_level.iter().enumerate() {
                if let Some(dir) = dir {
                    replaces_below |= index != upper && holds(dir, to_name)?;
                }
            }
        }
        let (holders, shadowed) = self.holders(&from_level, from_name)?;
        if holders
            .iter()
            .any(|&index| index != upper && self.layers[index].writable())
        {
            return Err(FilesystemError::CrossesDevices);
        }
        let read_only = holders
            .iter()
            .copied()
            .find(|&index| !self.layers[index].writable());
        let shown_below = self.whiteout_layer(read_only.into_iter().chain(shadowed).min())?;
        match moving.file_type {
            FileType::Directory if holders != [upper] => {
                return Err(FilesystemError::CrossesDevices)
            }
            FileType::File if holders.first() != Some(&upper) => {
                let shown = *holders.first().ok_or(FilesystemError::NotFound)?;
                self.copy_up_file(from, from_name, shown, upper)?
            }
            FileType::Symlink | FileType::Unknown if holders.first() != Some(&upper) => {
                return Err(FilesystemError::CrossesDevices)
            }
            _ => {}
        }
        let target_dir = self.upper_dir(to, upper)?;
        if let Some(upper) = shown_below {
            self.mark(from, &whiteout(from_name), upper)?;
        }
        let source_dir = from.level()[upper]
            .clone()
            .ok_or(FilesystemError::NotFound)?;
        let moving_dir = if moving.file_type == FileType::Directory {
            Some(source_dir.open_dir(from_name)?)
        } else {
            None
        };
        let marks = match &moving_dir {
            Some(dir) => replaces_below && !holds(dir, OPAQUE)?,
            None => false,
        };
        if let (true, Some(dir)) = (marks, &moving_dir) {
            put_marker(dir, OPAQUE)?;
        }
        let moved = || source_dir.rename(from_name, &target_dir, to_name, rename);
        let replaces_dir =
            matches!(replaced, Some((_, found)) if found.file_type == FileType::Directory);
        let moved = if replaces_dir && holds(&target_dir, to_name)? {
            let inner = target_dir.open_dir(to_name)?;
            without_markers(&inner, moved)
        } else {
            moved()
        };
        if let Err(err) = moved {
            if let (true, Some(dir)) = (marks, &moving_dir) {
                let _ = dir.remove_file(OPAQUE);
            }
            return Err(err);
        }
        Self::unhide(&target_dir, to_name);
        Ok(())
    }
}

/// The Layer's directory a level holds, which it holds wherever it shows
/// a name.
fn held(dir: &Option<Lower>) -> Result<&Lower, FilesystemError> {
    dir.as_ref().ok_or(FilesystemError::NotFound)
}
