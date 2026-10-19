//! The Layers of an Overlay and the rules that make them one view: which
//! Layer shows a path, how a change lands in a writable Layer, and the
//! markers that hide what a read-only Layer still holds.
//!
//! Each change that touches more than one entry writes its marker or its
//! replacement before it takes the old one away, as overlayfs builds a
//! copy before it swaps it in, so a change that fails part way leaves the
//! view as it was or as it was meant to become.

use std::sync::Arc;

use super::super::path::{ancestors, is_beneath, parent};
use super::super::{
    Access, DirEntry, File, Filesystem, FilesystemError, Kind, Metadata, OpenOptions, Rename,
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

    fn holds(&self, path: &str) -> bool {
        self.filesystem.metadata(path).is_ok()
    }

    /// Whether this Layer's markers hide `path` in every Layer below it:
    /// a whiteout of it or of a directory above it, or an opaque marker
    /// in one of those directories. A marker lives in a directory, so the
    /// walk ends at the first directory above `path` this Layer lacks.
    fn hides(&self, path: &str) -> bool {
        for name in ancestors(path).chain(std::iter::once(path)) {
            if self.holds(&whiteout(name)) || self.holds(&join(parent(name), OPAQUE)) {
                return true;
            }
            if name != path && !self.holds(name) {
                return false;
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
        target.write_at(&buf[..read], offset)?;
        offset += read as u64;
    }
}

/// Refuse a path naming an entry reserved for the Overlay's markers.
fn reserved(path: &str) -> Result<(), FilesystemError> {
    if path.split('/').any(|name| name.starts_with(WHITEOUT)) {
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

    /// The topmost Layer holding `path` and what it holds there, or `None`
    /// when no Layer shows it.
    fn locate(&self, path: &str) -> Result<Option<(usize, Metadata)>, FilesystemError> {
        self.locate_by(path, <dyn Filesystem>::metadata)
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

    /// Every Layer still showing or shadowing `path`, topmost first.
    fn holders(&self, path: &str) -> Vec<usize> {
        let mut holders = Vec::new();
        for (index, layer) in self.layers.iter().enumerate() {
            if layer.holds(path) {
                holders.push(index);
            } else if layer.hides(path) {
                break;
            }
        }
        holders
    }

    /// The topmost writable Layer, where every change that is not in place
    /// lands.
    fn upper(&self) -> Result<usize, FilesystemError> {
        self.layers
            .iter()
            .position(Layer::writable)
            .ok_or(FilesystemError::ReadOnly)
    }

    /// Create in Layer `into` each directory above `path` that so far shows
    /// only from another Layer.
    fn copy_up_directories(&self, path: &str, into: usize) -> Result<(), FilesystemError> {
        let target = &self.layers[into];
        for dir in ancestors(path) {
            if target.holds(dir) {
                continue;
            }
            match self.locate(dir)? {
                Some((
                    _,
                    Metadata {
                        kind: Kind::Directory,
                        ..
                    },
                )) => target.filesystem.create_dir(dir)?,
                Some(_) => return Err(FilesystemError::NotADirectory),
                None => return Err(FilesystemError::NotFound),
            }
        }
        Ok(())
    }

    /// Copy the file at `path` from Layer `from` into Layer `into` whole.
    /// Layer `into` held nothing there, so a copy that fails part way is
    /// taken away again rather than left to hide the whole file.
    fn copy_up_file(&self, path: &str, from: usize, into: usize) -> Result<(), FilesystemError> {
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
        let copied = copy_whole(source.as_mut(), copy.as_mut());
        drop(copy);
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
        self.layers[into]
            .filesystem
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

    /// The Layer a whiteout over `holders` lands in, or `None` when every
    /// one is writable; refused as read-only when no writable Layer sits
    /// above the topmost read-only one.
    fn whiteout_layer(&self, holders: &[usize]) -> Result<Option<usize>, FilesystemError> {
        match holders
            .iter()
            .find(|&&index| !self.layers[index].writable())
        {
            Some(&shown) => match self.upper()? {
                upper if upper < shown => Ok(Some(upper)),
                _ => Err(FilesystemError::ReadOnly),
            },
            None => Ok(None),
        }
    }

    /// Take `path` out of the view: a whiteout over any read-only Layer
    /// holding it first, then out of every writable Layer holding it.
    /// `remove` takes it out of one Layer.
    fn withdraw(
        &self,
        path: &str,
        remove: impl Fn(&dyn Filesystem, &str) -> Result<(), FilesystemError>,
    ) -> Result<(), FilesystemError> {
        let holders = self.holders(path);
        if let Some(upper) = self.whiteout_layer(&holders)? {
            self.mark(&whiteout(path), upper)?;
        }
        for &index in &holders {
            let layer = &self.layers[index];
            if layer.writable() {
                remove(layer.filesystem.as_ref(), path)?;
            }
        }
        Ok(())
    }

    pub(super) fn open(
        &self,
        path: &str,
        options: &OpenOptions,
    ) -> Result<Box<dyn File>, FilesystemError> {
        reserved(path)?;
        let changes = options.write || options.truncate;
        match self.locate(path)? {
            Some((
                _,
                Metadata {
                    kind: Kind::Directory,
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
                    .ok_or(FilesystemError::ReadOnly)?;
                self.copy_up_file(path, index, into)?;
                self.layers[into].filesystem.open(path, options)
            }
            // The new file shadows whatever the whiteout hid before the
            // whiteout goes, so nothing shows through in between.
            None if options.create || options.create_new => {
                let upper = self.upper()?;
                self.copy_up_directories(path, upper)?;
                let file = self.layers[upper].filesystem.open(path, options)?;
                self.unhide(path, upper)?;
                Ok(file)
            }
            None => Err(FilesystemError::NotFound),
        }
    }

    pub(super) fn metadata(&self, path: &str) -> Result<Metadata, FilesystemError> {
        reserved(path)?;
        self.locate(path)?
            .map(|(_, metadata)| metadata)
            .ok_or(FilesystemError::NotFound)
    }

    /// What the Layer showing `path` holds there, a link it holds
    /// reported as one even where it points at nothing.
    pub(super) fn symlink_metadata(&self, path: &str) -> Result<Metadata, FilesystemError> {
        reserved(path)?;
        self.locate_by(path, <dyn Filesystem>::symlink_metadata)?
            .map(|(_, metadata)| metadata)
            .ok_or(FilesystemError::NotFound)
    }

    pub(super) fn read_dir(&self, path: &str) -> Result<Vec<DirEntry>, FilesystemError> {
        if self.metadata(path)?.kind != Kind::Directory {
            return Err(FilesystemError::NotADirectory);
        }
        let mut entries: Vec<DirEntry> = Vec::new();
        let mut hidden: Vec<String> = Vec::new();
        for layer in &self.layers {
            match layer.find(path)? {
                Some(Metadata {
                    kind: Kind::Directory,
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
        reserved(path)?;
        if self.locate(path)?.is_some() {
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
    /// other entry there means the view is not what was found empty.
    pub(super) fn remove_dir(&self, path: &str) -> Result<(), FilesystemError> {
        if self.metadata(path)?.kind != Kind::Directory {
            return Err(FilesystemError::NotADirectory);
        }
        if !self.read_dir(path)?.is_empty() {
            return Err(FilesystemError::DirectoryNotEmpty);
        }
        self.withdraw(path, |filesystem, path| {
            for entry in filesystem.read_dir(path)? {
                if !entry.name.starts_with(WHITEOUT) {
                    return Err(FilesystemError::DirectoryNotEmpty);
                }
                filesystem.remove_file(&join(path, &entry.name))?;
            }
            filesystem.remove_dir(path)
        })
    }

    pub(super) fn remove_file(&self, path: &str) -> Result<(), FilesystemError> {
        if self.metadata(path)?.kind == Kind::Directory {
            return Err(FilesystemError::IsADirectory);
        }
        self.withdraw(path, |filesystem, path| filesystem.remove_file(path))
    }

    /// A directory moves only when the topmost writable Layer alone holds
    /// it; moving one out of another would mean copying up everything
    /// beneath it, so it is refused as crossing devices, as overlayfs
    /// refuses it without `redirect_dir`. What the view holds at `to` is
    /// checked as rename(2) checks it, and a directory moved over one a
    /// lower Layer holds is marked opaque. The source is whited out before
    /// it moves, so it never shows from a read-only Layer once moved.
    pub(super) fn rename(
        &self,
        from: &str,
        to: &str,
        rename: Rename,
    ) -> Result<(), FilesystemError> {
        reserved(to)?;
        let moving = self.metadata(from)?;
        if from == to {
            return Ok(());
        }
        rename.allows(moving.kind)?;
        if is_beneath(to, from) {
            return Err(FilesystemError::InvalidInput);
        }
        let replaced = self.locate(to)?.map(|(_, metadata)| metadata.kind);
        match (moving.kind, replaced) {
            (Kind::File, Some(Kind::Directory)) => return Err(FilesystemError::IsADirectory),
            (Kind::Directory, Some(Kind::File)) => return Err(FilesystemError::NotADirectory),
            (Kind::Directory, Some(Kind::Directory)) if !self.read_dir(to)?.is_empty() => {
                return Err(FilesystemError::DirectoryNotEmpty)
            }
            _ => {}
        }
        let upper = self.upper()?;
        let replaces_below = moving.kind == Kind::Directory
            && self
                .layers
                .iter()
                .enumerate()
                .any(|(index, layer)| index != upper && layer.holds(to));
        let holders = self.holders(from);
        let shown_below = self.whiteout_layer(&holders)?;
        match moving.kind {
            Kind::Directory if holders != [upper] => return Err(FilesystemError::CrossesDevices),
            Kind::File if holders.first() != Some(&upper) => {
                self.copy_up_file(from, holders[0], upper)?
            }
            _ => {}
        }
        self.copy_up_directories(to, upper)?;
        if let Some(upper) = shown_below {
            self.mark(&whiteout(from), upper)?;
        }
        self.layers[upper].filesystem.rename(from, to, rename)?;
        if replaces_below {
            self.mark(&join(to, OPAQUE), upper)?;
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
