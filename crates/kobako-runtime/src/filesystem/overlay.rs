//! `Overlay`: Layers stacked into one Filesystem, each read-only or
//! writable on its own.
//!
//! The rules follow the union filesystems that allow writable Layers
//! below the top, aufs among them. A removal hides what a read-only Layer
//! still shows with a whiteout, and a directory made again where one was
//! removed is marked opaque. Both markers take the OCI image layer
//! format's names and live in the topmost writable Layer, so they last as
//! long as it does.

use std::sync::Arc;

use super::path::{ancestors, is_beneath, parent};
use super::{DirEntry, File, Filesystem, FilesystemError, Kind, Metadata, OpenOptions};
use crate::mount::Access;

/// The prefix of a whiteout, and of every name reserved for a marker.
const WHITEOUT: &str = ".wh.";
/// The marker hiding every lower Layer's entries of its directory.
const OPAQUE: &str = ".wh..wh..opq";

/// Layers stacked into one Filesystem, the first one added on top.
#[derive(Clone, Default)]
pub struct Overlay {
    layers: Vec<Layer>,
}

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
        match self.filesystem.metadata(path) {
            Ok(metadata) => Ok(Some(metadata)),
            Err(FilesystemError::NotFound) => Ok(None),
            Err(err) => Err(err),
        }
    }

    fn holds(&self, path: &str) -> bool {
        self.filesystem.metadata(path).is_ok()
    }

    /// Whether this Layer's markers hide `path` in every Layer below it.
    fn hides(&self, path: &str) -> bool {
        let mut dirs = std::iter::once("").chain(ancestors(path));
        self.holds(&whiteout(path))
            || ancestors(path).any(|dir| self.holds(&whiteout(dir)))
            || dirs.any(|dir| self.holds(&join(dir, OPAQUE)))
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

/// Refuse a path naming an entry reserved for the Overlay's markers.
fn reserved(path: &str) -> Result<(), FilesystemError> {
    if path.split('/').any(|name| name.starts_with(WHITEOUT)) {
        return Err(FilesystemError::InvalidInput);
    }
    Ok(())
}

impl Overlay {
    /// An Overlay with no Layer yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// This Overlay with `filesystem` as a new Layer beneath the ones
    /// already added, allowing `access` there.
    pub fn layer(mut self, filesystem: Arc<dyn Filesystem>, access: Access) -> Self {
        self.layers.push(Layer { filesystem, access });
        self
    }

    /// The topmost Layer holding `path` and what it holds there, or `None`
    /// when no Layer shows it.
    fn locate(&self, path: &str) -> Result<Option<(usize, Metadata)>, FilesystemError> {
        for (index, layer) in self.layers.iter().enumerate() {
            if let Some(metadata) = layer.find(path)? {
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
    fn copy_up_file(&self, path: &str, from: usize, into: usize) -> Result<(), FilesystemError> {
        self.copy_up_directories(path, into)?;
        let mut source = self.layers[from].filesystem.open(
            path,
            &OpenOptions {
                read: true,
                ..OpenOptions::default()
            },
        )?;
        let mut target = self.layers[into].filesystem.open(
            path,
            &OpenOptions {
                write: true,
                create: true,
                truncate: true,
                ..OpenOptions::default()
            },
        )?;
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

    /// Clear the whiteout hiding `path` in Layer `into`, answering whether
    /// there was one.
    fn unhide(&self, path: &str, into: usize) -> Result<bool, FilesystemError> {
        let marker = whiteout(path);
        let layer = &self.layers[into];
        if !layer.holds(&marker) {
            return Ok(false);
        }
        layer.filesystem.remove_file(&marker)?;
        Ok(true)
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

    /// Take `path` out of the view: out of every writable Layer holding
    /// it, with a whiteout over any read-only one. `remove` takes it out of
    /// one Layer.
    fn withdraw(
        &self,
        path: &str,
        remove: impl Fn(&dyn Filesystem, &str) -> Result<(), FilesystemError>,
    ) -> Result<(), FilesystemError> {
        let holders = self.holders(path);
        let upper = self.whiteout_layer(&holders)?;
        for &index in &holders {
            let layer = &self.layers[index];
            if layer.writable() {
                remove(layer.filesystem.as_ref(), path)?;
            }
        }
        match upper {
            Some(upper) => self.mark(&whiteout(path), upper),
            None => Ok(()),
        }
    }
}

impl Filesystem for Overlay {
    fn open(&self, path: &str, options: &OpenOptions) -> Result<Box<dyn File>, FilesystemError> {
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
            None if options.create || options.create_new => {
                let upper = self.upper()?;
                self.copy_up_directories(path, upper)?;
                self.unhide(path, upper)?;
                self.layers[upper].filesystem.open(path, options)
            }
            None => Err(FilesystemError::NotFound),
        }
    }

    fn metadata(&self, path: &str) -> Result<Metadata, FilesystemError> {
        reserved(path)?;
        self.locate(path)?
            .map(|(_, metadata)| metadata)
            .ok_or(FilesystemError::NotFound)
    }

    fn read_dir(&self, path: &str) -> Result<Vec<DirEntry>, FilesystemError> {
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

    fn create_dir(&self, path: &str) -> Result<(), FilesystemError> {
        reserved(path)?;
        if self.locate(path)?.is_some() {
            return Err(FilesystemError::AlreadyExists);
        }
        let upper = self.upper()?;
        self.copy_up_directories(path, upper)?;
        let removed = self.unhide(path, upper)?;
        self.layers[upper].filesystem.create_dir(path)?;
        if removed {
            self.mark(&join(path, OPAQUE), upper)?;
        }
        Ok(())
    }

    fn remove_dir(&self, path: &str) -> Result<(), FilesystemError> {
        if self.metadata(path)?.kind != Kind::Directory {
            return Err(FilesystemError::NotADirectory);
        }
        if !self.read_dir(path)?.is_empty() {
            return Err(FilesystemError::DirectoryNotEmpty);
        }
        self.withdraw(path, |filesystem, path| {
            for marker in filesystem.read_dir(path)? {
                filesystem.remove_file(&join(path, &marker.name))?;
            }
            filesystem.remove_dir(path)
        })
    }

    fn remove_file(&self, path: &str) -> Result<(), FilesystemError> {
        if self.metadata(path)?.kind == Kind::Directory {
            return Err(FilesystemError::IsADirectory);
        }
        self.withdraw(path, |filesystem, path| filesystem.remove_file(path))
    }

    /// A directory moves only when the topmost writable Layer alone holds
    /// it, since moving it out of another would mean copying up everything
    /// beneath it.
    fn rename(&self, from: &str, to: &str) -> Result<(), FilesystemError> {
        reserved(to)?;
        let moving = self.metadata(from)?;
        if from == to {
            return Ok(());
        }
        if is_beneath(to, from) {
            return Err(FilesystemError::InvalidInput);
        }
        let upper = self.upper()?;
        let holders = self.holders(from);
        self.whiteout_layer(&holders)?;
        match moving.kind {
            Kind::Directory if holders != [upper] => return Err(FilesystemError::Unsupported),
            Kind::File if holders.first() != Some(&upper) => {
                self.copy_up_file(from, holders[0], upper)?
            }
            _ => {}
        }
        self.copy_up_directories(to, upper)?;
        self.unhide(to, upper)?;
        self.layers[upper].filesystem.rename(from, to)?;
        self.withdraw(from, |filesystem, path| filesystem.remove_file(path))
    }

    /// A fresh Overlay when any Layer starts each Invocation fresh, over
    /// each Layer's own next Filesystem.
    fn for_invocation(&self) -> Option<Arc<dyn Filesystem>> {
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
        Some(Arc::new(Self { layers }))
    }
}

#[cfg(test)]
mod tests {
    use super::super::memory::Memory;
    use super::super::temporary::Temporary;
    use super::*;

    fn read(fs: &dyn Filesystem, path: &str) -> Vec<u8> {
        let mut file = fs
            .open(
                path,
                &OpenOptions {
                    read: true,
                    ..OpenOptions::default()
                },
            )
            .expect("the file must open");
        let mut buf = vec![0; 64];
        let read = file.read_at(&mut buf, 0).expect("the file must read");
        buf.truncate(read);
        buf
    }

    fn write(fs: &dyn Filesystem, path: &str, contents: &[u8]) -> Result<(), FilesystemError> {
        fs.open(
            path,
            &OpenOptions {
                write: true,
                create: true,
                truncate: true,
                ..OpenOptions::default()
            },
        )?
        .write_at(contents, 0)
        .map(drop)
    }

    fn overwrite(fs: &dyn Filesystem, path: &str, contents: &[u8]) {
        write(fs, path, contents).expect("the write must land");
    }

    fn names(fs: &dyn Filesystem, path: &str) -> Vec<String> {
        let mut names: Vec<String> = fs
            .read_dir(path)
            .expect("the directory must list")
            .into_iter()
            .map(|entry| entry.name)
            .collect();
        names.sort();
        names
    }

    fn holding(path: &str, contents: &str) -> Arc<Memory> {
        Arc::new([(path, contents)].into_iter().collect())
    }

    /// A writable `upper` over a read-only Memory holding `dir/file` =
    /// "lower".
    fn layered(upper: Arc<dyn Filesystem>) -> (Overlay, Arc<Memory>) {
        let lower = holding("dir/file", "lower");
        let overlay = Overlay::new()
            .layer(upper, Access::ReadWrite)
            .layer(lower.clone(), Access::Read);
        (overlay, lower)
    }

    // @behavior FS-008
    #[test]
    fn an_overlay_reads_through_to_a_lower_layer() {
        let (overlay, _) = layered(Arc::new(Memory::new()));
        assert_eq!(
            read(&overlay, "dir/file"),
            b"lower",
            "a lower file read through Overlay::open must read back the lower content"
        );
    }

    // @behavior FS-009
    #[test]
    fn a_write_to_a_read_only_layers_file_lands_in_the_nearest_writable_layer_above_it() {
        let upper = Arc::new(Memory::new());
        let (overlay, _) = layered(upper.clone());
        overwrite(&overlay, "dir/file", b"upper");
        assert_eq!(
            read(upper.as_ref(), "dir/file"),
            b"upper",
            "a read-only Layer's file written through Overlay::open must land in the writable Layer above it"
        );
    }

    // @behavior FS-010
    #[test]
    fn an_overlay_leaves_a_read_only_layer_as_it_was() {
        let (overlay, lower) = layered(Arc::new(Memory::new()));
        overwrite(&overlay, "dir/file", b"upper");
        overlay
            .remove_file("dir/file")
            .expect("the file must be removed");
        assert_eq!(
            read(lower.as_ref(), "dir/file"),
            b"lower",
            "a read-only Layer's file written and removed through an Overlay must stay as it was"
        );
    }

    // @behavior FS-011
    #[test]
    fn a_file_removed_through_an_overlay_is_gone_from_its_view() {
        let (overlay, _) = layered(Arc::new(Memory::new()));
        overlay
            .remove_file("dir/file")
            .expect("the file must be removed");
        assert_eq!(
            overlay.metadata("dir/file"),
            Err(FilesystemError::NotFound),
            "a lower file removed through Overlay::remove_file must no longer be found there"
        );
    }

    // @behavior FS-012
    #[test]
    fn an_overlay_over_a_temporary_starts_every_invocation_from_its_lower_layers() {
        let (overlay, _) = layered(Arc::new(Temporary::new()));
        overwrite(&overlay, "dir/file", b"upper");
        let next = overlay
            .for_invocation()
            .expect("an Overlay over a Temporary must start each Invocation afresh");
        assert_eq!(
            read(next.as_ref(), "dir/file"),
            b"lower",
            "Overlay::for_invocation over a Temporary must hand the next Invocation the lower content"
        );
    }

    // @behavior FS-013
    #[test]
    fn an_upper_layer_hides_the_same_path_in_a_lower_one() {
        let overlay = Overlay::new()
            .layer(holding("note", "upper"), Access::Read)
            .layer(holding("note", "lower"), Access::Read);
        assert_eq!(
            read(&overlay, "note"),
            b"upper",
            "a path both Layers hold read through Overlay::open must read the upper Layer's content"
        );
    }

    // @behavior FS-014
    #[test]
    fn a_write_to_a_writable_layers_file_changes_it_in_place() {
        let upper = Arc::new(Memory::new());
        let lower = holding("note", "lower");
        let overlay = Overlay::new()
            .layer(upper.clone(), Access::ReadWrite)
            .layer(lower.clone(), Access::ReadWrite);
        overwrite(&overlay, "note", b"changed");
        assert_eq!(
            (read(lower.as_ref(), "note"), upper.metadata("note")),
            (b"changed".to_vec(), Err(FilesystemError::NotFound)),
            "a writable lower Layer's file written through Overlay::open must change in that Layer alone"
        );
    }

    // @behavior FS-015
    #[test]
    fn a_new_file_lands_in_the_topmost_writable_layer() {
        let first = Arc::new(Memory::new());
        let second = Arc::new(Memory::new());
        let overlay = Overlay::new()
            .layer(Arc::new(Memory::new()), Access::Read)
            .layer(first.clone(), Access::ReadWrite)
            .layer(second.clone(), Access::ReadWrite);
        overwrite(&overlay, "new", b"made");
        assert_eq!(
            (read(first.as_ref(), "new"), second.metadata("new")),
            (b"made".to_vec(), Err(FilesystemError::NotFound)),
            "a file created through Overlay::open must land in the topmost writable Layer"
        );
    }

    // @behavior FS-016
    #[test]
    fn a_removal_leaves_a_whiteout_in_the_topmost_writable_layer() {
        let upper = Arc::new(Memory::new());
        let (overlay, _) = layered(upper.clone());
        overlay
            .remove_file("dir/file")
            .expect("the file must be removed");
        assert_eq!(
            upper.metadata("dir/.wh.file").map(|metadata| metadata.kind),
            Ok(Kind::File),
            "a read-only Layer's file removed through Overlay::remove_file must leave .wh.file beside it in the writable Layer"
        );
    }

    // @behavior FS-017
    #[test]
    fn a_whiteout_lasts_as_long_as_its_writable_layer() {
        let upper = Arc::new(Memory::new());
        let (overlay, lower) = layered(upper.clone());
        overlay
            .remove_file("dir/file")
            .expect("the file must be removed");
        let rebuilt = Overlay::new()
            .layer(upper, Access::ReadWrite)
            .layer(lower, Access::Read);
        assert_eq!(
            rebuilt.metadata("dir/file"),
            Err(FilesystemError::NotFound),
            "a lower file removed through an Overlay over a Memory must stay gone from a new Overlay over the same Layers"
        );
    }

    // @behavior FS-018
    #[test]
    fn a_directory_made_again_where_one_was_removed_shows_nothing_from_below() {
        let (overlay, _) = layered(Arc::new(Memory::new()));
        overlay
            .remove_file("dir/file")
            .expect("the file must be removed");
        overlay
            .remove_dir("dir")
            .expect("the directory must be removed");
        overlay
            .create_dir("dir")
            .expect("the directory must be made");
        assert_eq!(
            names(&overlay, "dir"),
            Vec::<String>::new(),
            "a directory made again through Overlay::create_dir must list nothing from the read-only Layer"
        );
    }

    // @behavior FS-019
    #[test]
    fn an_overlay_lists_none_of_its_markers() {
        let (overlay, _) = layered(Arc::new(Memory::new()));
        overwrite(&overlay, "dir/new", b"made");
        overlay
            .remove_file("dir/file")
            .expect("the file must be removed");
        assert_eq!(
            names(&overlay, "dir"),
            ["new"],
            "a directory holding a whiteout listed through Overlay::read_dir must show no .wh. entry"
        );
    }

    // @behavior FS-020
    #[test]
    fn an_overlay_refuses_a_name_reserved_for_its_markers() {
        let (overlay, _) = layered(Arc::new(Memory::new()));
        assert_eq!(
            write(&overlay, "dir/.wh.note", b"x"),
            Err(FilesystemError::InvalidInput),
            "a file named .wh.note created through Overlay::open must be refused"
        );
    }

    // @behavior FS-021
    #[test]
    fn an_overlay_with_no_writable_layer_refuses_every_change_as_read_only() {
        let overlay = Overlay::new().layer(holding("dir/file", "lower"), Access::Read);
        assert_eq!(
            write(&overlay, "dir/file", b"x"),
            Err(FilesystemError::ReadOnly),
            "a file written through an Overlay of read-only Layers must be refused as read-only"
        );
    }

    #[test]
    fn an_overlay_lists_every_layer_without_what_it_hid() {
        let (overlay, _) = layered(Arc::new(Memory::new()));
        overwrite(&overlay, "dir/new", b"made");
        assert_eq!(names(&overlay, "dir"), ["file", "new"]);
    }

    #[test]
    fn a_lower_file_renamed_through_an_overlay_moves_into_the_writable_layer() {
        let upper = Arc::new(Memory::new());
        let (overlay, lower) = layered(upper.clone());
        overlay
            .rename("dir/file", "dir/moved")
            .expect("the file must move");
        assert_eq!(
            (
                read(&overlay, "dir/moved"),
                overlay.metadata("dir/file"),
                read(lower.as_ref(), "dir/file"),
            ),
            (
                b"lower".to_vec(),
                Err(FilesystemError::NotFound),
                b"lower".to_vec()
            ),
        );
    }
}
