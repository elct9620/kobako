//! The Layers of an Overlay and the rules that make them one view: which
//! Layer shows a path, how a change lands in a writable Layer, and the
//! markers that hide what a read-only Layer still holds.
//!
//! Each change that touches more than one entry writes its marker or its
//! replacement before it takes the old one away, as overlayfs builds a
//! copy before it swaps it in, so a change that fails part way leaves the
//! view as it was or as it was meant to become.

use std::sync::Arc;

use super::super::path::{ancestors, beneath_itself, is_beneath, parent};
use super::super::{
    Access, DirEntry, File, FileType, Filesystem, FilesystemError, Metadata, OpenOptions, Rename,
};

/// The prefix of a whiteout, and of every name reserved for a marker.
const WHITEOUT: &str = ".wh.";
/// The marker hiding every lower Layer's entries of its directory.
const OPAQUE: &str = ".wh..wh..opq";

/// How a Layer is asked what a path names: following a link there, or not.
type Lookup = fn(&(dyn Filesystem + 'static), &str) -> Result<Metadata, FilesystemError>;

#[derive(Clone)]
struct Layer {
    filesystem: Arc<dyn Filesystem>,
    access: Access,
}

impl Layer {
    fn writable(&self) -> bool {
        self.access == Access::ReadWrite
    }

    fn find(&self, path: &str) -> Result<Option<Metadata>, FilesystemError> {
        self.find_by(path, <dyn Filesystem>::metadata)
    }

    fn find_by(&self, path: &str, lookup: Lookup) -> Result<Option<Metadata>, FilesystemError> {
        match lookup(self.filesystem.as_ref(), path) {
            Ok(metadata) => Ok(Some(metadata)),
            Err(FilesystemError::NotFound) => Ok(None),
            Err(err) => Err(err),
        }
    }

    /// Whether this Layer has an entry at `path`, a link pointing at
    /// nothing among them.
    fn holds(&self, path: &str) -> bool {
        self.filesystem.symlink_metadata(path).is_ok()
    }

    /// Whether this Layer hides `path` in every Layer below it: a whiteout
    /// of it or of a directory above it, an opaque marker in one of those
    /// directories or in `path` itself, or a file or link standing at it
    /// or above it, as overlayfs merges only directories. A marker lives
    /// in a directory, so the walk ends at the first directory above
    /// `path` this Layer lacks.
    fn hides(&self, path: &str) -> bool {
        if self.holds(&join(path, OPAQUE)) {
            return true;
        }
        for name in ancestors(path).chain(std::iter::once(path)) {
            if self.holds(&whiteout(name)) || self.holds(&join(parent(name), OPAQUE)) {
                return true;
            }
            match self.filesystem.symlink_metadata(name) {
                Ok(metadata) if metadata.file_type != FileType::Directory => return true,
                Ok(_) => {}
                Err(_) if name != path => return false,
                Err(_) => {}
            }
        }
        false
    }
}

/// The whiteout hiding `path`, beside it in its directory.
fn whiteout(path: &str) -> String {
    let name = path.rsplit('/').next().unwrap_or(path);
    join(parent(path), &format!("{WHITEOUT}{name}"))
}

fn join(dir: &str, name: &str) -> String {
    if dir.is_empty() {
        name.to_string()
    } else {
        format!("{dir}/{name}")
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

/// Take every marker out of a writable Layer's copy of the directory
/// `path`, which the view shows empty, for `act` to change it; any other
/// entry there means it is not. When `act` fails, or a marker will not
/// go, the markers taken out are put back, so the view stays as it was.
fn without_markers(
    filesystem: &dyn Filesystem,
    path: &str,
    act: impl FnOnce() -> Result<(), FilesystemError>,
) -> Result<(), FilesystemError> {
    let markers: Vec<String> = filesystem
        .read_dir(path)?
        .into_iter()
        .map(|entry| entry.name)
        .collect();
    if markers.iter().any(|name| !name.starts_with(WHITEOUT)) {
        return Err(FilesystemError::DirectoryNotEmpty);
    }
    let mut cleared = Vec::new();
    let changed = markers
        .iter()
        .try_for_each(|name| {
            let marker = join(path, name);
            filesystem.remove_file(&marker)?;
            cleared.push(marker);
            Ok(())
        })
        .and_then(|()| act());
    if changed.is_err() {
        for marker in cleared {
            let _ = put_marker(filesystem, &marker);
        }
    }
    changed
}

/// An empty file at `path`, as every marker is.
fn put_marker(filesystem: &dyn Filesystem, path: &str) -> Result<(), FilesystemError> {
    filesystem
        .open(
            path,
            &OpenOptions {
                write: true,
                create: true,
                ..OpenOptions::default()
            },
        )?
        .close()
}

/// Refuse a path naming an entry reserved for the Overlay's markers, in
/// any case, since a Layer that folds case would hold `.WH.name` as one.
fn refuse_reserved(path: &str) -> Result<(), FilesystemError> {
    let marker = |name: &str| {
        name.get(..WHITEOUT.len())
            .is_some_and(|start| start.eq_ignore_ascii_case(WHITEOUT))
    };
    if path.split('/').any(marker) {
        return Err(FilesystemError::InvalidInput);
    }
    Ok(())
}

/// Layers stacked into one view, the first added on top.
#[derive(Clone, Default)]
pub(super) struct Stack {
    layers: Vec<Layer>,
}

impl Stack {
    pub(super) fn push(&mut self, filesystem: Arc<dyn Filesystem>, access: Access) {
        self.layers.push(Layer { filesystem, access });
    }

    /// The Layers a new Invocation starts from, or `None` when every Layer
    /// is shared.
    pub(super) fn for_invocation(&self) -> Option<Self> {
        let next: Vec<_> = self
            .layers
            .iter()
            .map(|layer| layer.filesystem.for_invocation())
            .collect();
        if next.iter().all(Option::is_none) {
            return None;
        }
        let layers = self
            .layers
            .iter()
            .zip(next)
            .map(|(layer, next)| Layer {
                filesystem: next.unwrap_or_else(|| layer.filesystem.clone()),
                access: layer.access,
            })
            .collect();
        Some(Self { layers })
    }

    /// The topmost Layer holding `path` and what it holds there, a link
    /// followed within that Layer, or `None` when no Layer shows it or the
    /// link shown points at nothing.
    fn locate(&self, path: &str) -> Result<Option<(usize, Metadata)>, FilesystemError> {
        match self.locate_by(path, <dyn Filesystem>::symlink_metadata)? {
            Some((index, metadata)) if metadata.file_type == FileType::Symlink => Ok(self.layers
                [index]
                .find(path)?
                .map(|metadata| (index, metadata))),
            found => Ok(found),
        }
    }

    fn locate_by(
        &self,
        path: &str,
        lookup: Lookup,
    ) -> Result<Option<(usize, Metadata)>, FilesystemError> {
        for (index, layer) in self.layers.iter().enumerate() {
            if let Some(metadata) = layer.find_by(path, lookup)? {
                return Ok(Some((index, metadata)));
            }
            if layer.hides(path) {
                break;
            }
        }
        Ok(None)
    }

    /// Every Layer still showing `path`, topmost first, and the topmost
    /// Layer below them whose entry there a file or link above shadows.
    fn holders(&self, path: &str) -> (Vec<usize>, Option<usize>) {
        let mut holders = Vec::new();
        for (index, layer) in self.layers.iter().enumerate() {
            if layer.holds(path) {
                holders.push(index);
            }
            if layer.hides(path) {
                let shadowed = layer
                    .holds(path)
                    .then(|| self.beneath(index, path))
                    .flatten();
                return (holders, shadowed);
            }
        }
        (holders, None)
    }

    /// The topmost Layer below `above` holding `path` in sight.
    fn beneath(&self, above: usize, path: &str) -> Option<usize> {
        for (index, layer) in self.layers.iter().enumerate().skip(above + 1) {
            if layer.holds(path) {
                return Some(index);
            }
            if layer.hides(path) {
                return None;
            }
        }
        None
    }

    /// The topmost writable Layer, where every change that is not in place
    /// lands.
    fn upper(&self) -> Result<usize, FilesystemError> {
        self.layers
            .iter()
            .position(Layer::writable)
            .ok_or(FilesystemError::ReadOnlyFilesystem)
    }

    /// Create in Layer `into` each directory above `path` that so far shows
    /// only from another Layer. A Layer holds no link it was not handed, so
    /// a change reaching through a link another Layer holds is refused as
    /// crossing devices rather than landing beside where the link points.
    fn copy_up_directories(&self, path: &str, into: usize) -> Result<(), FilesystemError> {
        let target = &self.layers[into];
        for dir in ancestors(path) {
            if target.holds(dir) {
                continue;
            }
            match self.locate_by(dir, <dyn Filesystem>::symlink_metadata)? {
                Some((_, metadata)) => match metadata.file_type {
                    FileType::Directory => target.filesystem.create_dir(dir)?,
                    FileType::Symlink => return Err(FilesystemError::CrossesDevices),
                    _ => return Err(FilesystemError::NotADirectory),
                },
                None => return Err(FilesystemError::NotFound),
            }
        }
        Ok(())
    }

    /// Copy the file at `path` from Layer `from` into Layer `into` whole.
    /// Layer `into` held nothing there, so a copy that fails part way is
    /// taken away again rather than left to hide the whole file. A link
    /// is not copied up, as no Layer is handed one to hold.
    fn copy_up_file(&self, path: &str, from: usize, into: usize) -> Result<(), FilesystemError> {
        if self.layers[from]
            .filesystem
            .symlink_metadata(path)?
            .file_type
            == FileType::Symlink
        {
            return Err(FilesystemError::CrossesDevices);
        }
        self.copy_up_directories(path, into)?;
        let mut source = self.layers[from].filesystem.open(
            path,
            &OpenOptions {
                read: true,
                ..OpenOptions::default()
            },
        )?;
        let target = &self.layers[into].filesystem;
        let mut copy = target.open(
            path,
            &OpenOptions {
                write: true,
                create: true,
                truncate: true,
                ..OpenOptions::default()
            },
        )?;
        let copied = copy_whole(source.as_mut(), copy.as_mut()).and(copy.close());
        if copied.is_err() {
            let _ = target.remove_file(path);
        }
        copied
    }

    /// Clear the whiteout hiding `path` in Layer `into`.
    fn unhide(&self, path: &str, into: usize) -> Result<(), FilesystemError> {
        let marker = whiteout(path);
        let layer = &self.layers[into];
        if layer.holds(&marker) {
            layer.filesystem.remove_file(&marker)?;
        }
        Ok(())
    }

    fn mark(&self, path: &str, into: usize) -> Result<(), FilesystemError> {
        self.copy_up_directories(path, into)?;
        put_marker(self.layers[into].filesystem.as_ref(), path)
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

    /// Take `path` out of the view: a whiteout over any Layer that keeps it
    /// first, then out of every other Layer holding it. A read-only Layer
    /// keeps it, and so does a writable one `keeps` answers for, so no
    /// Layer changes before each is known to go; `remove` takes it out of
    /// one Layer.
    fn withdraw(
        &self,
        path: &str,
        keeps: impl Fn(&dyn Filesystem) -> Result<bool, FilesystemError>,
        remove: impl Fn(&dyn Filesystem, &str) -> Result<(), FilesystemError>,
    ) -> Result<(), FilesystemError> {
        let (holders, shadowed) = self.holders(path);
        let mut kept = Vec::new();
        for &index in &holders {
            let layer = &self.layers[index];
            if !layer.writable() || keeps(layer.filesystem.as_ref())? {
                kept.push(index);
            }
        }
        let shown = kept.first().copied().into_iter().chain(shadowed).min();
        if let Some(upper) = self.whiteout_layer(shown)? {
            self.mark(&whiteout(path), upper)?;
        }
        for &index in &holders {
            if !kept.contains(&index) {
                remove(self.layers[index].filesystem.as_ref(), path)?;
            }
        }
        Ok(())
    }

    pub(super) fn open(
        &self,
        path: &str,
        options: &OpenOptions,
    ) -> Result<Box<dyn File>, FilesystemError> {
        refuse_reserved(path)?;
        let changes = options.write || options.truncate;
        match self.locate(path)? {
            Some((
                _,
                Metadata {
                    file_type: FileType::Directory,
                    ..
                },
            )) => Err(FilesystemError::IsADirectory),
            Some(_) if options.create_new => Err(FilesystemError::AlreadyExists),
            Some((index, _)) if !changes || self.layers[index].writable() => {
                self.layers[index].filesystem.open(path, options)
            }
            Some((index, _)) => {
                let into = self.layers[..index]
                    .iter()
                    .rposition(Layer::writable)
                    .ok_or(FilesystemError::ReadOnlyFilesystem)?;
                self.copy_up_file(path, index, into)?;
                self.layers[into].filesystem.open(path, options)
            }
            None if options.create || options.create_new => self.create_file(path, options),
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
        path: &str,
        options: &OpenOptions,
    ) -> Result<Box<dyn File>, FilesystemError> {
        match self.locate_by(path, <dyn Filesystem>::symlink_metadata)? {
            Some(_) if options.create_new => Err(FilesystemError::AlreadyExists),
            Some((index, _)) if self.layers[index].writable() => {
                self.layers[index].filesystem.open(path, options)
            }
            Some(_) => Err(FilesystemError::CrossesDevices),
            None => {
                let upper = self.upper()?;
                self.copy_up_directories(path, upper)?;
                let file = self.layers[upper].filesystem.open(path, options)?;
                self.unhide(path, upper)?;
                Ok(file)
            }
        }
    }

    pub(super) fn metadata(&self, path: &str) -> Result<Metadata, FilesystemError> {
        refuse_reserved(path)?;
        let (index, metadata) = self.locate(path)?.ok_or(FilesystemError::NotFound)?;
        self.merged(path, index, metadata)
    }

    /// What the Layer showing `path` holds there, a link it holds
    /// reported as one even where it points at nothing.
    pub(super) fn symlink_metadata(&self, path: &str) -> Result<Metadata, FilesystemError> {
        refuse_reserved(path)?;
        let (index, metadata) = self
            .locate_by(path, <dyn Filesystem>::symlink_metadata)?
            .ok_or(FilesystemError::NotFound)?;
        self.merged(path, index, metadata)
    }

    /// `metadata`, found at `path` in the Layer at `index`, as the merged
    /// view reports it: a directory takes the identity of the lowest Layer
    /// still showing it, so copying it up into a writable Layer leaves the
    /// identity as it was.
    fn merged(
        &self,
        path: &str,
        index: usize,
        metadata: Metadata,
    ) -> Result<Metadata, FilesystemError> {
        if metadata.file_type != FileType::Directory {
            return Ok(metadata);
        }
        let mut lowest = metadata;
        for layer in &self.layers[index..] {
            match layer.find(path)? {
                Some(found) if found.file_type == FileType::Directory => lowest = found,
                Some(_) => break,
                None => {}
            }
            if layer.hides(path) {
                break;
            }
        }
        Ok(metadata.with_identity(lowest.dev, lowest.ino))
    }

    pub(super) fn read_dir(&self, path: &str) -> Result<Vec<DirEntry>, FilesystemError> {
        if self.metadata(path)?.file_type != FileType::Directory {
            return Err(FilesystemError::NotADirectory);
        }
        let mut entries: Vec<DirEntry> = Vec::new();
        let mut hidden: Vec<String> = Vec::new();
        for layer in &self.layers {
            match layer.find(path)? {
                Some(Metadata {
                    file_type: FileType::Directory,
                    ..
                }) => {
                    let listed = layer.filesystem.read_dir(path)?;
                    for entry in &listed {
                        let shown = !entry.name.starts_with(WHITEOUT)
                            && !hidden.contains(&entry.name)
                            && !entries.iter().any(|seen| seen.name == entry.name);
                        if shown {
                            entries.push(entry.clone());
                        }
                    }
                    hidden.extend(
                        listed
                            .iter()
                            .filter_map(|entry| entry.name.strip_prefix(WHITEOUT))
                            .map(str::to_string),
                    );
                    if listed.iter().any(|entry| entry.name == OPAQUE) {
                        break;
                    }
                }
                Some(_) => break,
                None => {}
            }
            if layer.hides(path) {
                break;
            }
        }
        Ok(entries)
    }

    /// A directory made where a whiteout hid one is marked opaque before
    /// the whiteout goes, so the lower Layers' entries never show in it.
    pub(super) fn create_dir(&self, path: &str) -> Result<(), FilesystemError> {
        refuse_reserved(path)?;
        if self
            .locate_by(path, <dyn Filesystem>::symlink_metadata)?
            .is_some()
        {
            return Err(FilesystemError::AlreadyExists);
        }
        let upper = self.upper()?;
        self.copy_up_directories(path, upper)?;
        let layer = &self.layers[upper];
        let hid = layer.holds(&whiteout(path));
        layer.filesystem.create_dir(path)?;
        if hid {
            if let Err(err) = self.mark(&join(path, OPAQUE), upper) {
                let _ = layer.filesystem.remove_dir(path);
                return Err(err);
            }
            self.unhide(path, upper)?;
        }
        Ok(())
    }

    /// A writable Layer's copy of the directory may hold only markers; any
    /// other entry there means the view is not what was found empty. A
    /// copy still holding what a Layer above hides stays, beneath the
    /// whiteout.
    pub(super) fn remove_dir(&self, path: &str) -> Result<(), FilesystemError> {
        if self.symlink_metadata(path)?.file_type != FileType::Directory {
            return Err(FilesystemError::NotADirectory);
        }
        if !self.read_dir(path)?.is_empty() {
            return Err(FilesystemError::DirectoryNotEmpty);
        }
        self.withdraw(
            path,
            |filesystem| {
                Ok(filesystem
                    .read_dir(path)?
                    .iter()
                    .any(|entry| !entry.name.starts_with(WHITEOUT)))
            },
            |filesystem, path| without_markers(filesystem, path, || filesystem.remove_dir(path)),
        )
    }

    /// A link is removed as itself, wherever it points.
    pub(super) fn remove_file(&self, path: &str) -> Result<(), FilesystemError> {
        if self.symlink_metadata(path)?.file_type == FileType::Directory {
            return Err(FilesystemError::IsADirectory);
        }
        self.withdraw(
            path,
            |_| Ok(false),
            |filesystem, path| filesystem.remove_file(path),
        )
    }

    /// A directory moves only when the topmost writable Layer alone holds
    /// it; moving one out of another would mean copying up everything
    /// beneath it, so it is refused as crossing devices, as overlayfs
    /// refuses it without `redirect_dir`. What the view holds at `to` is
    /// checked as rename(2) checks it, and a directory moved over one a
    /// lower Layer holds is marked opaque before it moves, so it never
    /// lands without the marker. The source is whited out before it moves,
    /// so it never shows from a read-only Layer once moved.
    pub(super) fn rename(
        &self,
        from: &str,
        to: &str,
        rename: Rename,
    ) -> Result<(), FilesystemError> {
        refuse_reserved(to)?;
        let moving = self.symlink_metadata(from)?;
        if from == to {
            return Ok(());
        }
        rename.check(moving.file_type)?;
        if is_beneath(to, from) {
            return Err(beneath_itself(moving.file_type));
        }
        let replaced = self
            .locate_by(to, <dyn Filesystem>::symlink_metadata)?
            .map(|(_, metadata)| metadata.file_type);
        match (moving.file_type, replaced) {
            (FileType::File | FileType::Symlink, Some(FileType::Directory)) => {
                return Err(FilesystemError::IsADirectory)
            }
            (FileType::Directory, Some(FileType::File | FileType::Symlink)) => {
                return Err(FilesystemError::NotADirectory)
            }
            (FileType::Directory, Some(FileType::Directory)) if !self.read_dir(to)?.is_empty() => {
                return Err(FilesystemError::DirectoryNotEmpty)
            }
            _ => {}
        }
        let upper = self.upper()?;
        let replaces_below = moving.file_type == FileType::Directory
            && self
                .layers
                .iter()
                .enumerate()
                .any(|(index, layer)| index != upper && layer.holds(to));
        let (holders, shadowed) = self.holders(from);
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
                self.copy_up_file(from, holders[0], upper)?
            }
            FileType::Symlink if holders.first() != Some(&upper) => {
                return Err(FilesystemError::CrossesDevices)
            }
            _ => {}
        }
        self.copy_up_directories(to, upper)?;
        if let Some(upper) = shown_below {
            self.mark(&whiteout(from), upper)?;
        }
        let filesystem = self.layers[upper].filesystem.as_ref();
        let opaque = join(from, OPAQUE);
        let marks = replaces_below && !self.layers[upper].holds(&opaque);
        if marks {
            put_marker(filesystem, &opaque)?;
        }
        let moved = || filesystem.rename(from, to, rename);
        let moved = if replaced == Some(FileType::Directory) && self.layers[upper].holds(to) {
            without_markers(filesystem, to, moved)
        } else {
            moved()
        };
        if let Err(err) = moved {
            if marks {
                let _ = filesystem.remove_file(&opaque);
            }
            return Err(err);
        }
        self.unhide(to, upper)?;
        for &index in &holders {
            let layer = &self.layers[index];
            if index != upper && layer.writable() {
                layer.filesystem.remove_file(from)?;
            }
        }
        Ok(())
    }
}
