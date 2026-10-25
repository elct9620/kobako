//! `Overlay`: Layers stacked into one Filesystem, each read-only or
//! writable on its own.
//!
//! The rules follow the union filesystems that allow writable Layers
//! below the top, aufs among them. A removal hides what a read-only Layer
//! still shows with a whiteout, and a directory made again where one was
//! removed is marked opaque. Both markers take the OCI image layer
//! format's names and live in the topmost writable Layer, so they last as
//! long as it does. Every call holds the Overlay's lock, so a change
//! across several Layers is never seen half made.

mod exclusive;
mod stack;

use std::sync::Arc;

pub use exclusive::Blocking;

use super::{
    Access, DirEntry, File, Filesystem, FilesystemError, Hold, Metadata, OpenOptions, Rename,
};
use exclusive::{Exclusive, Inline};
use stack::Stack;

/// Layers stacked into one Filesystem, the first one added on top. Its
/// clones share its lock; two Overlays sharing a writable Layer are not
/// serialized against each other, as overlayfs leaves two mounts sharing
/// an upper directory undefined.
#[derive(Clone)]
pub struct Overlay {
    stack: Stack,
    lock: Arc<Exclusive>,
}

impl Default for Overlay {
    fn default() -> Self {
        Self::new()
    }
}

impl Overlay {
    /// An Overlay with no Layer yet.
    pub fn new() -> Self {
        Self {
            stack: Stack::default(),
            lock: Arc::new(Exclusive::new(Arc::new(Inline))),
        }
    }

    /// This Overlay with `filesystem` as a new Layer beneath the ones
    /// already added, allowing `access` there. It takes a lock of its own,
    /// so an Overlay holding the one it was built from as a Layer does not
    /// wait on itself.
    pub fn layer(mut self, filesystem: Arc<dyn Filesystem>, access: Access) -> Self {
        self.stack.push(filesystem, access);
        self.lock = Arc::new(Exclusive::new(self.lock.blocking()));
        self
    }

    /// This Overlay waiting for its lock through `blocking`.
    pub fn with_blocking(self, blocking: Arc<dyn Blocking>) -> Self {
        Self {
            lock: Arc::new(Exclusive::new(blocking)),
            ..self
        }
    }
}

impl Filesystem for Overlay {
    fn open(&self, path: &str, options: &OpenOptions) -> Result<Box<dyn File>, FilesystemError> {
        let _held = self.lock.hold()?;
        self.stack.open(path, options)
    }

    fn metadata(&self, path: &str) -> Result<Metadata, FilesystemError> {
        let _held = self.lock.hold()?;
        self.stack.metadata(path)
    }

    fn symlink_metadata(&self, path: &str) -> Result<Metadata, FilesystemError> {
        let _held = self.lock.hold()?;
        self.stack.symlink_metadata(path)
    }

    fn stored_path(&self, path: &str) -> Result<String, FilesystemError> {
        let _held = self.lock.hold()?;
        self.stack.stored_path(path)
    }

    fn hold_directory(&self, path: &str) -> Result<Option<Hold>, FilesystemError> {
        let _held = self.lock.hold()?;
        self.stack.hold_directory(path)
    }

    fn read_dir(&self, path: &str) -> Result<Vec<DirEntry>, FilesystemError> {
        let _held = self.lock.hold()?;
        self.stack.read_dir(path)
    }

    fn create_dir(&self, path: &str) -> Result<(), FilesystemError> {
        let _held = self.lock.hold()?;
        self.stack.create_dir(path)
    }

    fn remove_dir(&self, path: &str) -> Result<(), FilesystemError> {
        let _held = self.lock.hold()?;
        self.stack.remove_dir(path)
    }

    fn remove_file(&self, path: &str) -> Result<(), FilesystemError> {
        let _held = self.lock.hold()?;
        self.stack.remove_file(path)
    }

    fn rename(&self, from: &str, to: &str, rename: Rename) -> Result<(), FilesystemError> {
        let _held = self.lock.hold()?;
        self.stack.rename(from, to, rename)
    }

    /// A fresh Overlay when any Layer starts each Invocation fresh, over
    /// each Layer's own next Filesystem. It keeps this Overlay's lock,
    /// since the Layers it shares are the same.
    fn for_invocation(&self) -> Option<Arc<dyn Filesystem>> {
        let stack = self.stack.for_invocation()?;
        Some(Arc::new(Self {
            stack,
            lock: self.lock.clone(),
        }))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::Mutex;
    use std::thread;
    use std::time::{Duration, Instant};

    use super::super::memory::Memory;
    use super::super::temporary::Temporary;
    use super::super::FileType;
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

    fn memory_with(path: &str, contents: &str) -> Arc<Memory> {
        Arc::new([(path, contents)].into_iter().collect())
    }

    /// A writable `upper` over a read-only Memory holding `dir/file` =
    /// "lower".
    fn overlay_on(upper: Arc<dyn Filesystem>) -> (Overlay, Arc<Memory>) {
        let lower = memory_with("dir/file", "lower");
        let overlay = Overlay::new()
            .layer(upper, Access::ReadWrite)
            .layer(lower.clone(), Access::Read);
        (overlay, lower)
    }

    /// A host directory holding `target`, `dir/inner`, and the links `link`
    /// to `target`, `dirlink` to `dir`, and `dangling` to nothing, removed
    /// when it drops.
    #[cfg(unix)]
    struct LinkTree(std::path::PathBuf);

    #[cfg(unix)]
    impl LinkTree {
        fn new(name: &str) -> Self {
            let path =
                std::env::temp_dir().join(format!("kobako-overlay-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(path.join("dir")).expect("the host directory must be created");
            std::fs::write(path.join("target"), "lower").expect("the host file must be written");
            std::fs::write(path.join("dir/inner"), "lower").expect("the host file must be written");
            for (target, link) in [
                ("target", "link"),
                ("dir", "dirlink"),
                ("missing", "dangling"),
            ] {
                std::os::unix::fs::symlink(target, path.join(link)).expect("the link must be made");
            }
            Self(path)
        }

        /// A writable Memory over the host directory as a read-only Layer.
        fn overlay(&self) -> Overlay {
            let lower =
                super::super::Directory::new(&self.0).expect("the host directory must open");
            Overlay::new()
                .layer(Arc::new(Memory::new()), Access::ReadWrite)
                .layer(Arc::new(lower), Access::Read)
        }
    }

    #[cfg(unix)]
    impl Drop for LinkTree {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    // The link points at a file in the same Layer, so only a lookup that
    // reaches the Layer's own `symlink_metadata` can answer a link.
    // @behavior FS-049
    #[cfg(unix)]
    #[test]
    fn an_overlay_reports_a_link_its_layer_holds_as_a_link() {
        let host = LinkTree::new("lstat");
        let overlay = host.overlay();
        let file_type = |path| {
            overlay
                .symlink_metadata(path)
                .map(|metadata| metadata.file_type)
        };
        assert_eq!(
            (file_type("link"), file_type("dangling")),
            (Ok(FileType::Symlink), Ok(FileType::Symlink)),
            "a link a Directory Layer holds, pointing at a file or at nothing, looked up through \
             Overlay::symlink_metadata must be reported as a link"
        );
    }

    // A link to a directory follows to one, and a dangling link follows to
    // nothing, so only judging each link itself lets both go.
    // @behavior FS-050
    #[cfg(unix)]
    #[test]
    fn an_overlay_removes_a_link_its_read_only_layer_holds_as_itself() {
        let host = LinkTree::new("unlink");
        let overlay = host.overlay();
        let removed = (
            overlay.remove_dir("dirlink"),
            overlay.remove_file("dirlink"),
            overlay.remove_file("dangling"),
        );
        let gone = |path| overlay.symlink_metadata(path).err();
        assert_eq!(
            (
                removed,
                gone("dirlink"),
                gone("dangling"),
                host.0.join("dirlink").is_symlink()
            ),
            (
                (Err(FilesystemError::NotADirectory), Ok(()), Ok(())),
                Some(FilesystemError::NotFound),
                Some(FilesystemError::NotFound),
                true
            ),
            "a link a read-only Directory Layer holds, removed through Overlay::remove_dir and \
             Overlay::remove_file, must be refused as no directory and then removed as itself, \
             leaving the Layer as it was"
        );
    }

    // A write would copy the FIFO up, which opens it, and a rename would
    // move what no Layer above can hold, so only refusals keep either
    // from waiting on a peer or landing nothing.
    // @behavior FS-092
    #[cfg(unix)]
    #[test]
    fn an_overlay_neither_copies_up_nor_moves_a_fifo_below_it() {
        let host = LinkTree::new("fifo");
        let made = std::process::Command::new("mkfifo")
            .arg(host.0.join("pipe"))
            .status()
            .expect("mkfifo must run");
        assert!(made.success(), "the FIFO must be made");
        let overlay = host.overlay();
        assert_eq!(
            (
                write(&overlay, "pipe", b"upper"),
                overlay.rename("pipe", "moved", Rename::File)
            ),
            (
                Err(FilesystemError::PermissionDenied),
                Err(FilesystemError::CrossesDevices)
            ),
            "a FIFO a read-only Directory Layer holds, written or renamed through an Overlay, must \
             be refused as permission denied and as crossing devices"
        );
    }

    // Each change would land in the writable Memory, which holds no link,
    // so only refusing it keeps the write from parting with its target.
    // @behavior FS-051
    #[cfg(unix)]
    #[test]
    fn an_overlay_refuses_a_change_through_a_link_its_read_only_layer_holds() {
        let host = LinkTree::new("through");
        let overlay = host.overlay();
        let creates = OpenOptions {
            write: true,
            create: true,
            ..OpenOptions::default()
        };
        let refused = [
            write(&overlay, "link", b"upper"),
            overlay.open("dirlink/new", &creates).map(drop),
            overlay.open("dangling", &creates).map(drop),
            overlay.rename("link", "moved", Rename::Any),
        ];
        assert_eq!(
            (refused, read(&overlay, "target")),
            (
                [
                    Err(FilesystemError::CrossesDevices),
                    Err(FilesystemError::CrossesDevices),
                    Err(FilesystemError::CrossesDevices),
                    Err(FilesystemError::CrossesDevices)
                ],
                b"lower".to_vec()
            ),
            "a write, a create, and a rename reaching through a link a read-only Directory Layer \
             holds, through an Overlay, must be refused as crossing devices and leave the target \
             as it was"
        );
    }

    // The lower Memory holds a readable `dangling`, so only the upper
    // link standing at that path can keep the read from reaching it.
    // @behavior FS-053
    #[cfg(unix)]
    #[test]
    fn a_link_a_layer_holds_hides_what_the_layers_below_hold_there() {
        let host = LinkTree::new("shadow");
        let upper = super::super::Directory::new(&host.0).expect("the host directory must open");
        let overlay = Overlay::new()
            .layer(Arc::new(upper), Access::ReadWrite)
            .layer(memory_with("dangling", "lower"), Access::Read);
        assert_eq!(
            overlay
                .open(
                    "dangling",
                    &OpenOptions {
                        read: true,
                        ..OpenOptions::default()
                    }
                )
                .map(drop),
            Err(FilesystemError::NotFound),
            "a lower file beneath a dangling link a higher Directory Layer holds at its path, \
             opened through Overlay::open, must not be found"
        );
    }

    // The lower Memory holds `dirlink/x` beside where the upper link points,
    // so only the link hiding everything beneath it keeps `x` out.
    // @behavior FS-057
    #[cfg(unix)]
    #[test]
    fn a_link_a_layer_holds_hides_what_the_layers_below_hold_beneath_it() {
        let host = LinkTree::new("shadow-beneath");
        let upper = super::super::Directory::new(&host.0).expect("the host directory must open");
        let overlay = Overlay::new()
            .layer(Arc::new(upper), Access::ReadWrite)
            .layer(memory_with("dirlink/x", "lower"), Access::Read);
        let found = overlay
            .metadata("dirlink/x")
            .map(|metadata| metadata.file_type);
        assert_eq!(
            (names(&overlay, "dirlink"), found),
            (vec!["inner".to_string()], Err(FilesystemError::NotFound)),
            "a directory a higher Directory Layer holds as a link, listed and looked into through \
             an Overlay, must show only where the link points and none of a lower Layer's entries"
        );
    }

    // The lower directory sits in a writable Layer, so only the whiteout,
    // not a removal that cannot take a directory, keeps the view whole.
    // @behavior FS-058
    #[test]
    fn removing_a_file_over_a_lower_directory_leaves_it_hidden() {
        let lower = memory_with("f/g", "lower");
        let overlay = Overlay::new()
            .layer(memory_with("f", "upper"), Access::ReadWrite)
            .layer(lower.clone(), Access::ReadWrite);
        let removed = overlay.remove_file("f");
        assert_eq!(
            (
                removed,
                overlay.symlink_metadata("f").err(),
                lower.metadata("f/g").is_ok()
            ),
            (Ok(()), Some(FilesystemError::NotFound), true),
            "a file a higher Layer holds over a lower Layer's directory, removed through \
             Overlay::remove_file, must leave the path gone from the view and the lower \
             directory as it was"
        );
    }

    // The writable Layer's `dir` holds only the whiteout hiding `file`, so
    // only judging the view, not that copy, lets the move go.
    // @behavior FS-059
    #[test]
    fn a_directory_moves_onto_one_holding_only_markers() {
        let (overlay, _) = overlay_on(Arc::new(Memory::new()));
        overlay
            .remove_file("dir/file")
            .expect("the file must be removed");
        overlay
            .create_dir("new")
            .expect("the directory must be made");
        overwrite(&overlay, "new/made", b"made");
        let moved = overlay.rename("new", "dir", Rename::Directory);
        assert_eq!(
            (moved, names(&overlay, "dir")),
            (Ok(()), vec!["made".to_string()]),
            "a directory renamed through Overlay::rename onto one the view shows empty, whose \
             writable copy holds only a whiteout, must take its place and show only its own entries"
        );
    }

    // The directory made again is marked opaque in the writable Layer, so
    // only that mark tells it apart from one the read-only Layer still holds.
    // @behavior FS-060
    #[test]
    fn a_directory_made_again_moves_as_the_writable_layers_own() {
        let (overlay, _) = overlay_on(Arc::new(Memory::new()));
        overlay
            .remove_file("dir/file")
            .expect("the file must be removed");
        overlay
            .remove_dir("dir")
            .expect("the directory must be removed");
        overlay
            .create_dir("dir")
            .expect("the directory must be made again");
        let moved = overlay.rename("dir", "moved", Rename::Directory);
        assert_eq!(
            (
                moved,
                overlay.metadata("dir").err(),
                overlay.read_dir("moved").map(|entries| entries.len())
            ),
            (Ok(()), Some(FilesystemError::NotFound), Ok(0)),
            "a directory made again through an Overlay where a read-only Layer's one was removed, \
             renamed through Overlay::rename, must move and leave nothing of the lower one behind"
        );
    }

    // @behavior FS-061
    #[test]
    fn an_overlay_refuses_a_file_moved_beneath_itself_as_no_directory() {
        let (overlay, _) = overlay_on(Arc::new(Memory::new()));
        assert_eq!(
            overlay.rename("dir/file", "dir/file/x", Rename::Any),
            Err(FilesystemError::NotADirectory),
            "a file renamed through Overlay::rename to a path beneath itself must be refused as \
             no directory, as rename(2) refuses it"
        );
    }

    // The link points at nothing, so only the link itself can be what an
    // exclusive create or a new directory finds there.
    // @behavior FS-052
    #[cfg(unix)]
    #[test]
    fn an_overlay_finds_a_dangling_link_where_a_new_entry_would_go() {
        let host = LinkTree::new("dangling");
        let overlay = host.overlay();
        let exclusive = OpenOptions {
            write: true,
            create_new: true,
            ..OpenOptions::default()
        };
        assert_eq!(
            (
                overlay.open("dangling", &exclusive).map(drop),
                overlay.create_dir("dangling")
            ),
            (
                Err(FilesystemError::AlreadyExists),
                Err(FilesystemError::AlreadyExists)
            ),
            "an exclusive create and a create_dir at a dangling link a read-only Directory Layer \
             holds, through an Overlay, must find it already there"
        );
    }

    // @behavior FS-008
    #[test]
    fn an_overlay_reads_through_to_a_lower_layer() {
        let (overlay, _) = overlay_on(Arc::new(Memory::new()));
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
        let (overlay, _) = overlay_on(upper.clone());
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
        let (overlay, lower) = overlay_on(Arc::new(Memory::new()));
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
        let (overlay, _) = overlay_on(Arc::new(Memory::new()));
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
        let (overlay, _) = overlay_on(Arc::new(Temporary::new()));
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
            .layer(memory_with("note", "upper"), Access::Read)
            .layer(memory_with("note", "lower"), Access::Read);
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
        let lower = memory_with("note", "lower");
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

    // The lower Layer is writable, so only the refusal itself keeps the
    // move from landing in two Layers.
    // @behavior FS-088
    #[test]
    fn an_overlay_refuses_to_move_a_file_a_lower_writable_layer_holds() {
        let upper = Arc::new(Memory::new());
        let lower = memory_with("note", "lower");
        let overlay = Overlay::new()
            .layer(upper.clone(), Access::ReadWrite)
            .layer(lower.clone(), Access::ReadWrite);
        assert_eq!(
            (
                overlay.rename("note", "moved", Rename::File),
                read(lower.as_ref(), "note"),
                upper.read_dir("").map(|entries| entries.len())
            ),
            (Err(FilesystemError::CrossesDevices), b"lower".to_vec(), Ok(0)),
            "a file a lower writable Layer holds, renamed through Overlay::rename, must be refused \
             as crossing devices with nothing moved or copied"
        );
    }

    // The writable Layer keeps the whiteout the removal left, so only a
    // call that answers by what landed rather than by the clean-up after
    // it succeeds.
    // @behavior FS-089
    #[test]
    fn an_overlay_change_that_lands_succeeds_though_its_whiteout_stays() {
        let (overlay, _) = overlay_on(Arc::new(KeepsWhiteouts(Memory::new())));
        overlay
            .remove_file("dir/file")
            .expect("the file must be removed");
        assert_eq!(
            write(&overlay, "dir/file", b"again").map(|()| read(&overlay, "dir/file")),
            Ok(b"again".to_vec()),
            "a file made again through Overlay::open where it was removed, while the writable \
             Layer refuses to remove the whiteout, must succeed and show the new file"
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
        let (overlay, _) = overlay_on(upper.clone());
        overlay
            .remove_file("dir/file")
            .expect("the file must be removed");
        assert_eq!(
            upper.metadata("dir/.wh.file").map(|metadata| metadata.file_type),
            Ok(FileType::File),
            "a read-only Layer's file removed through Overlay::remove_file must leave .wh.file beside it in the writable Layer"
        );
    }

    // @behavior FS-017
    #[test]
    fn a_whiteout_lasts_as_long_as_its_writable_layer() {
        let upper = Arc::new(Memory::new());
        let (overlay, lower) = overlay_on(upper.clone());
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
        let (overlay, _) = overlay_on(Arc::new(Memory::new()));
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
        let (overlay, _) = overlay_on(Arc::new(Memory::new()));
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
        let (overlay, _) = overlay_on(Arc::new(Memory::new()));
        assert_eq!(
            write(&overlay, "dir/.wh.note", b"x"),
            Err(FilesystemError::InvalidInput),
            "a file named .wh.note created through Overlay::open must be refused"
        );
    }

    // A Layer folding case would hold `.WH.note` as the marker hiding `note`.
    // @behavior FS-056
    #[test]
    fn an_overlay_refuses_its_reserved_name_in_any_case() {
        let (overlay, _) = overlay_on(Arc::new(Memory::new()));
        assert_eq!(
            write(&overlay, "dir/.WH.note", b"x"),
            Err(FilesystemError::InvalidInput),
            "a file named .WH.note created through Overlay::open must be refused"
        );
    }

    // @behavior FS-021
    #[test]
    fn an_overlay_with_no_writable_layer_refuses_every_change_as_read_only() {
        let overlay = Overlay::new().layer(memory_with("dir/file", "lower"), Access::Read);
        assert_eq!(
            write(&overlay, "dir/file", b"x"),
            Err(FilesystemError::ReadOnlyFilesystem),
            "a file written through an Overlay of read-only Layers must be refused as read-only"
        );
    }

    #[test]
    fn an_overlay_lists_every_layer_without_what_it_hid() {
        let (overlay, _) = overlay_on(Arc::new(Memory::new()));
        overwrite(&overlay, "dir/new", b"made");
        assert_eq!(names(&overlay, "dir"), ["file", "new"]);
    }

    #[test]
    fn a_lower_file_renamed_through_an_overlay_moves_into_the_writable_layer() {
        let upper = Arc::new(Memory::new());
        let (overlay, lower) = overlay_on(upper.clone());
        overlay
            .rename("dir/file", "dir/moved", Rename::Any)
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

    /// A Memory that, the first time it lists a directory, starts another
    /// call through `overlay` on a thread of its own and gives it a while
    /// to finish, recording whether it did before this call returned.
    struct Interleaves {
        memory: Memory,
        overlay: Mutex<Option<Overlay>>,
        finished_inside: Mutex<Option<bool>>,
        other: Mutex<Option<thread::JoinHandle<()>>>,
    }

    impl Filesystem for Interleaves {
        fn open(
            &self,
            path: &str,
            options: &OpenOptions,
        ) -> Result<Box<dyn File>, FilesystemError> {
            self.memory.open(path, options)
        }

        fn metadata(&self, path: &str) -> Result<Metadata, FilesystemError> {
            self.memory.metadata(path)
        }

        fn read_dir(&self, path: &str) -> Result<Vec<DirEntry>, FilesystemError> {
            if let Some(overlay) = self.overlay.lock().unwrap().take() {
                let finished = Arc::new(AtomicUsize::new(0));
                let marker = finished.clone();
                let handle = thread::spawn(move || {
                    let _ = write(&overlay, "dir/new", b"made");
                    marker.store(1, Ordering::SeqCst);
                });
                let deadline = Instant::now() + Duration::from_millis(200);
                while finished.load(Ordering::SeqCst) == 0 && Instant::now() < deadline {
                    thread::sleep(Duration::from_millis(5));
                }
                *self.finished_inside.lock().unwrap() = Some(finished.load(Ordering::SeqCst) == 1);
                *self.other.lock().unwrap() = Some(handle);
            }
            self.memory.read_dir(path)
        }

        fn create_dir(&self, path: &str) -> Result<(), FilesystemError> {
            self.memory.create_dir(path)
        }

        fn remove_dir(&self, path: &str) -> Result<(), FilesystemError> {
            self.memory.remove_dir(path)
        }

        fn remove_file(&self, path: &str) -> Result<(), FilesystemError> {
            self.memory.remove_file(path)
        }

        fn rename(&self, from: &str, to: &str, rename: Rename) -> Result<(), FilesystemError> {
            self.memory.rename(from, to, rename)
        }
    }

    // The other call starts inside this one, between its emptiness check
    // and its removal, so only the Overlay's lock keeps the file it
    // creates from being removed with the directory.
    // @behavior FS-037
    #[test]
    fn a_change_through_an_overlay_waits_for_the_one_in_progress() {
        let upper = Arc::new(Interleaves {
            memory: [("dir/.keep", "")].into_iter().collect(),
            overlay: Mutex::new(None),
            finished_inside: Mutex::new(None),
            other: Mutex::new(None),
        });
        upper
            .memory
            .remove_file("dir/.keep")
            .expect("the seed must go");
        let overlay = Overlay::new().layer(upper.clone(), Access::ReadWrite);
        *upper.overlay.lock().unwrap() = Some(overlay.clone());
        let removed = overlay.remove_dir("dir");
        upper
            .other
            .lock()
            .unwrap()
            .take()
            .expect("the other call must have started")
            .join()
            .expect("the other call must not panic");
        assert_eq!(
            (removed, *upper.finished_inside.lock().unwrap(), overlay.metadata("dir/new").is_ok()),
            (Ok(()), Some(false), false),
            "a change through Overlay::open started while Overlay::remove_dir runs must wait for it, \
             failing rather than landing in the directory being removed"
        );
    }

    // @behavior FS-038
    #[test]
    fn a_call_through_an_overlay_from_inside_one_answers_a_deadlock() {
        struct Reenters {
            memory: Memory,
            overlay: Mutex<Option<Overlay>>,
            answer: Mutex<Option<Result<Metadata, FilesystemError>>>,
        }

        impl Filesystem for Reenters {
            fn open(
                &self,
                path: &str,
                options: &OpenOptions,
            ) -> Result<Box<dyn File>, FilesystemError> {
                self.memory.open(path, options)
            }

            fn metadata(&self, path: &str) -> Result<Metadata, FilesystemError> {
                if let Some(overlay) = self.overlay.lock().unwrap().take() {
                    *self.answer.lock().unwrap() = Some(overlay.metadata(path));
                }
                self.memory.metadata(path)
            }

            fn read_dir(&self, path: &str) -> Result<Vec<DirEntry>, FilesystemError> {
                self.memory.read_dir(path)
            }

            fn create_dir(&self, path: &str) -> Result<(), FilesystemError> {
                self.memory.create_dir(path)
            }

            fn remove_dir(&self, path: &str) -> Result<(), FilesystemError> {
                self.memory.remove_dir(path)
            }

            fn remove_file(&self, path: &str) -> Result<(), FilesystemError> {
                self.memory.remove_file(path)
            }

            fn rename(&self, from: &str, to: &str, rename: Rename) -> Result<(), FilesystemError> {
                self.memory.rename(from, to, rename)
            }
        }

        let layer = Arc::new(Reenters {
            memory: [("note", "x")].into_iter().collect(),
            overlay: Mutex::new(None),
            answer: Mutex::new(None),
        });
        let overlay = Overlay::new().layer(layer.clone(), Access::Read);
        *layer.overlay.lock().unwrap() = Some(overlay.clone());
        overlay
            .metadata("note")
            .expect("the outer call must answer");
        assert_eq!(
            layer.answer.lock().unwrap().take(),
            Some(Err(FilesystemError::Deadlock)),
            "a call through an Overlay made from inside one of its own calls on the same thread \
             must answer Deadlock rather than wait forever"
        );
    }

    /// A writable Memory seeded with `files` and room for `inodes`, over a
    /// read-only one holding `dir/file` = "lower".
    fn full_over_lower(files: &[(&str, &str)], inodes: usize) -> (Overlay, Arc<Memory>) {
        let upper = Memory::from_iter(files.iter().copied()).with_inodes(Some(inodes));
        let upper = Arc::new(upper);
        let overlay = Overlay::new()
            .layer(upper.clone(), Access::ReadWrite)
            .layer(memory_with("dir/file", "lower"), Access::Read);
        (overlay, upper)
    }

    // The upper copy is all the room there is, so the whiteout cannot be
    // made; taking the copy away first would show the lower file again.
    // @behavior FS-040
    #[test]
    fn a_removal_that_cannot_leave_its_whiteout_removes_nothing() {
        let (overlay, _) = full_over_lower(&[("dir/file", "upper")], 3);
        assert_eq!(
            (overlay.remove_file("dir/file"), read(&overlay, "dir/file")),
            (Err(FilesystemError::StorageFull), b"upper".to_vec()),
            "a file removed through Overlay::remove_file whose whiteout finds no room must stay as \
             it was"
        );
    }

    // @behavior FS-041
    #[test]
    fn a_directory_that_cannot_be_marked_opaque_is_not_made() {
        let (overlay, upper) = full_over_lower(&[(".wh.dir", "")], 3);
        assert_eq!(
            (
                overlay.create_dir("dir"),
                overlay.metadata("dir"),
                upper.metadata("dir")
            ),
            (
                Err(FilesystemError::StorageFull),
                Err(FilesystemError::NotFound),
                Err(FilesystemError::NotFound)
            ),
            "a directory made through Overlay::create_dir where one was removed, with no room for \
             its opaque marker, must not be made, nor its whiteout lifted"
        );
    }

    // The copy up takes the last room, so the whiteout over the source
    // cannot be made; moving first would leave the file in two places.
    // @behavior FS-042
    #[test]
    fn a_rename_that_cannot_leave_its_whiteout_moves_nothing() {
        let (overlay, _) = full_over_lower(&[], 3);
        assert_eq!(
            (
                overlay.rename("dir/file", "dir/moved", Rename::Any),
                overlay.metadata("dir/moved"),
                read(&overlay, "dir/file")
            ),
            (
                Err(FilesystemError::StorageFull),
                Err(FilesystemError::NotFound),
                b"lower".to_vec()
            ),
            "a file renamed through Overlay::rename whose whiteout finds no room must stay where \
             it was"
        );
    }

    // The lower writable copy still holds the file the upper whiteout
    // hides, so it cannot be removed; removing the upper copy first would
    // show the file again.
    // @behavior FS-074
    #[test]
    fn a_directory_a_lower_writable_layer_still_fills_is_removed_behind_a_whiteout() {
        let lower = memory_with("dir/file", "lower");
        let overlay = Overlay::new()
            .layer(memory_with("dir/.wh.file", ""), Access::ReadWrite)
            .layer(lower.clone(), Access::ReadWrite);
        assert_eq!(
            (
                overlay.remove_dir("dir"),
                overlay.metadata("dir").err(),
                read(lower.as_ref(), "dir/file")
            ),
            (Ok(()), Some(FilesystemError::NotFound), b"lower".to_vec()),
            "a directory the view shows empty, removed through Overlay::remove_dir while a lower \
             writable Layer still holds a whited-out file in it, must leave the view and stay \
             below unseen"
        );
    }

    // The marker is the one entry past the room the upper Layer has, so
    // marking after the move would leave the directory moved.
    // @behavior FS-075
    #[test]
    fn a_rename_that_cannot_mark_its_directory_opaque_moves_nothing() {
        let upper = Arc::new(Memory::new().with_inodes(Some(2)));
        upper
            .create_dir("moving")
            .expect("the directory must be made");
        let lower = Arc::new(Memory::new());
        lower.create_dir("dir").expect("the directory must be made");
        let overlay = Overlay::new()
            .layer(upper.clone(), Access::ReadWrite)
            .layer(lower, Access::Read);
        assert_eq!(
            (
                overlay.rename("moving", "dir", Rename::Directory),
                upper.metadata("moving").map(|metadata| metadata.file_type),
                upper.metadata("dir").err()
            ),
            (
                Err(FilesystemError::StorageFull),
                Ok(FileType::Directory),
                Some(FilesystemError::NotFound)
            ),
            "a directory renamed through Overlay::rename over one a lower Layer holds, with no \
             room for its opaque marker, must stay where it was"
        );
    }

    // The writable Layer sits between two read-only ones, so a rename
    // landing in it beneath the top Layer's `to` would vanish from view.
    // @behavior FS-076
    #[test]
    fn a_rename_a_read_only_layer_above_would_cover_is_refused() {
        let overlay = Overlay::new()
            .layer(memory_with("to", "top"), Access::Read)
            .layer(Arc::new(Memory::new()), Access::ReadWrite)
            .layer(memory_with("from", "moved"), Access::Read);
        assert_eq!(
            (
                overlay.rename("from", "to", Rename::Any),
                read(&overlay, "from"),
                read(&overlay, "to")
            ),
            (
                Err(FilesystemError::ReadOnlyFilesystem),
                b"moved".to_vec(),
                b"top".to_vec()
            ),
            "a file renamed through Overlay::rename onto a name a read-only Layer above the \
             writable one holds must be refused as read-only and move nothing"
        );
    }

    // @behavior FS-077
    #[test]
    fn a_name_a_read_only_layer_above_hides_is_not_made() {
        let upper = Arc::new(Memory::new());
        let overlay = Overlay::new()
            .layer(
                Arc::new(
                    [(".wh.file", ""), (".wh.dir", "")]
                        .into_iter()
                        .collect::<Memory>(),
                ),
                Access::Read,
            )
            .layer(upper.clone(), Access::ReadWrite);
        assert_eq!(
            (
                write(&overlay, "file", b"new"),
                overlay.create_dir("dir"),
                upper.metadata("file").err(),
                upper.metadata("dir").err()
            ),
            (
                Err(FilesystemError::ReadOnlyFilesystem),
                Err(FilesystemError::ReadOnlyFilesystem),
                Some(FilesystemError::NotFound),
                Some(FilesystemError::NotFound)
            ),
            "a file and a directory made through an Overlay where a read-only Layer above the \
             writable one holds their whiteouts must be refused as read-only and not made"
        );
    }

    // @behavior FS-043
    #[test]
    fn a_directory_a_lower_layer_holds_does_not_move_through_an_overlay() {
        let (overlay, _) = overlay_on(Arc::new(Memory::new()));
        assert_eq!(
            (
                overlay.rename("dir", "moved", Rename::Any),
                overlay.metadata("moved")
            ),
            (
                Err(FilesystemError::CrossesDevices),
                Err(FilesystemError::NotFound)
            ),
            "a directory a read-only Layer holds renamed through Overlay::rename must be refused \
             as crossing devices, as overlayfs refuses it"
        );
    }

    // The directory shows only from the read-only Layer, so only a check of
    // the whole view finds it in the way.
    // @behavior FS-044
    #[test]
    fn a_rename_onto_what_an_overlay_shows_is_checked_against_the_view() {
        let upper = Arc::new(Memory::new());
        let (overlay, _) = overlay_on(upper.clone());
        overwrite(&overlay, "note", b"x");
        overlay
            .create_dir("empty")
            .expect("the directory must be made");
        assert_eq!(
            (
                overlay.rename("note", "dir", Rename::Any),
                overlay.rename("empty", "dir", Rename::Any),
                read(&overlay, "dir/file")
            ),
            (
                Err(FilesystemError::IsADirectory),
                Err(FilesystemError::DirectoryNotEmpty),
                b"lower".to_vec()
            ),
            "a rename through Overlay::rename onto a directory only a read-only Layer shows must \
             be refused as rename(2) refuses it, leaving that directory as it was"
        );
    }

    // A directory moved over an empty one a lower Layer holds must not take
    // up what that Layer later shows there.
    // @behavior FS-045
    #[test]
    fn a_directory_moved_over_a_lower_one_is_marked_opaque() {
        let upper = Arc::new(Memory::new());
        let lower: Arc<Memory> = Arc::new([("dir/.keep", "")].into_iter().collect());
        lower.remove_file("dir/.keep").expect("the seed must go");
        let overlay = Overlay::new()
            .layer(upper.clone(), Access::ReadWrite)
            .layer(lower.clone(), Access::Read);
        overlay
            .create_dir("made")
            .expect("the directory must be made");
        overlay
            .rename("made", "dir", Rename::Any)
            .expect("the directory must move over the empty one");
        assert_eq!(
            upper
                .metadata("dir/.wh..wh..opq")
                .map(|metadata| metadata.file_type),
            Ok(FileType::File),
            "a directory moved through Overlay::rename over one a read-only Layer holds must be \
             marked opaque in the writable Layer"
        );
    }

    // The writable Layer has room for the copy's entry but for less than
    // the lower file's bytes, so the copy fails part way.
    // @behavior FS-046
    #[test]
    fn a_copy_up_that_fails_part_way_leaves_nothing_behind() {
        let upper = Arc::new(Memory::new().with_limit(Some(2)).with_inodes(Some(8)));
        let (overlay, _) = overlay_on(upper.clone());
        assert_eq!(
            (
                write(&overlay, "dir/file", b"x"),
                upper.metadata("dir/file"),
                read(&overlay, "dir/file")
            ),
            (
                Err(FilesystemError::StorageFull),
                Err(FilesystemError::NotFound),
                b"lower".to_vec()
            ),
            "a write through Overlay::open whose copy up runs out of room must leave no partial \
             copy hiding the lower file"
        );
    }

    /// A Memory whose files write at most three bytes per call and, when
    /// `refuses_close`, refuse their close.
    struct Trickles {
        memory: Memory,
        refuses_close: bool,
    }

    struct TrickleFile {
        file: Box<dyn File>,
        refuses_close: bool,
    }

    impl File for TrickleFile {
        fn read_at(&mut self, buf: &mut [u8], offset: u64) -> Result<usize, FilesystemError> {
            self.file.read_at(buf, offset)
        }

        fn write_at(&mut self, buf: &[u8], offset: u64) -> Result<usize, FilesystemError> {
            self.file.write_at(&buf[..buf.len().min(3)], offset)
        }

        fn metadata(&self) -> Result<Metadata, FilesystemError> {
            self.file.metadata()
        }

        fn set_len(&mut self, len: u64) -> Result<(), FilesystemError> {
            self.file.set_len(len)
        }

        fn close(self: Box<Self>) -> Result<(), FilesystemError> {
            if self.refuses_close {
                return Err(FilesystemError::PermissionDenied);
            }
            self.file.close()
        }
    }

    impl Filesystem for Trickles {
        fn open(
            &self,
            path: &str,
            options: &OpenOptions,
        ) -> Result<Box<dyn File>, FilesystemError> {
            Ok(Box::new(TrickleFile {
                file: self.memory.open(path, options)?,
                refuses_close: self.refuses_close,
            }))
        }

        fn metadata(&self, path: &str) -> Result<Metadata, FilesystemError> {
            self.memory.metadata(path)
        }

        fn read_dir(&self, path: &str) -> Result<Vec<DirEntry>, FilesystemError> {
            self.memory.read_dir(path)
        }

        fn create_dir(&self, path: &str) -> Result<(), FilesystemError> {
            self.memory.create_dir(path)
        }

        fn remove_dir(&self, path: &str) -> Result<(), FilesystemError> {
            self.memory.remove_dir(path)
        }

        fn remove_file(&self, path: &str) -> Result<(), FilesystemError> {
            self.memory.remove_file(path)
        }

        fn rename(&self, from: &str, to: &str, rename: Rename) -> Result<(), FilesystemError> {
            self.memory.rename(from, to, rename)
        }
    }

    /// A Memory refusing to move or remove a directory.
    struct HoldsDirectories(Memory);

    impl Filesystem for HoldsDirectories {
        fn open(
            &self,
            path: &str,
            options: &OpenOptions,
        ) -> Result<Box<dyn File>, FilesystemError> {
            self.0.open(path, options)
        }

        fn metadata(&self, path: &str) -> Result<Metadata, FilesystemError> {
            self.0.metadata(path)
        }

        fn read_dir(&self, path: &str) -> Result<Vec<DirEntry>, FilesystemError> {
            self.0.read_dir(path)
        }

        fn create_dir(&self, path: &str) -> Result<(), FilesystemError> {
            self.0.create_dir(path)
        }

        fn remove_dir(&self, _path: &str) -> Result<(), FilesystemError> {
            Err(FilesystemError::PermissionDenied)
        }

        fn remove_file(&self, path: &str) -> Result<(), FilesystemError> {
            self.0.remove_file(path)
        }

        fn rename(&self, _from: &str, _to: &str, _rename: Rename) -> Result<(), FilesystemError> {
            Err(FilesystemError::PermissionDenied)
        }
    }

    /// A Memory refusing to remove a whiteout.
    struct KeepsWhiteouts(Memory);

    impl Filesystem for KeepsWhiteouts {
        fn open(
            &self,
            path: &str,
            options: &OpenOptions,
        ) -> Result<Box<dyn File>, FilesystemError> {
            self.0.open(path, options)
        }

        fn metadata(&self, path: &str) -> Result<Metadata, FilesystemError> {
            self.0.metadata(path)
        }

        fn read_dir(&self, path: &str) -> Result<Vec<DirEntry>, FilesystemError> {
            self.0.read_dir(path)
        }

        fn create_dir(&self, path: &str) -> Result<(), FilesystemError> {
            self.0.create_dir(path)
        }

        fn remove_dir(&self, path: &str) -> Result<(), FilesystemError> {
            self.0.remove_dir(path)
        }

        fn remove_file(&self, path: &str) -> Result<(), FilesystemError> {
            match path.rsplit('/').next() {
                Some(name) if name.starts_with(".wh.") => Err(FilesystemError::PermissionDenied),
                _ => self.0.remove_file(path),
            }
        }

        fn rename(&self, from: &str, to: &str, rename: Rename) -> Result<(), FilesystemError> {
            self.0.rename(from, to, rename)
        }
    }

    /// A Memory answering a lookup of any marker name with an I/O failure.
    struct ReadsNoMarker(Memory);

    impl Filesystem for ReadsNoMarker {
        fn open(
            &self,
            path: &str,
            options: &OpenOptions,
        ) -> Result<Box<dyn File>, FilesystemError> {
            self.0.open(path, options)
        }

        fn metadata(&self, path: &str) -> Result<Metadata, FilesystemError> {
            if path
                .rsplit('/')
                .next()
                .is_some_and(|name| name.starts_with(".wh."))
            {
                return Err(FilesystemError::Other(
                    "the marker cannot be read".to_string(),
                ));
            }
            self.0.metadata(path)
        }

        fn read_dir(&self, path: &str) -> Result<Vec<DirEntry>, FilesystemError> {
            self.0.read_dir(path)
        }

        fn create_dir(&self, path: &str) -> Result<(), FilesystemError> {
            self.0.create_dir(path)
        }

        fn remove_dir(&self, path: &str) -> Result<(), FilesystemError> {
            self.0.remove_dir(path)
        }

        fn remove_file(&self, path: &str) -> Result<(), FilesystemError> {
            self.0.remove_file(path)
        }

        fn rename(&self, from: &str, to: &str, rename: Rename) -> Result<(), FilesystemError> {
            self.0.rename(from, to, rename)
        }
    }

    /// A Memory whose `file` is gone after its first lookup, as a host
    /// directory another process changes mid-call.
    struct Vanishes {
        memory: Memory,
        seen: AtomicBool,
    }

    impl Filesystem for Vanishes {
        fn open(
            &self,
            path: &str,
            options: &OpenOptions,
        ) -> Result<Box<dyn File>, FilesystemError> {
            self.memory.open(path, options)
        }

        fn metadata(&self, path: &str) -> Result<Metadata, FilesystemError> {
            if path == "file" && self.seen.swap(true, Ordering::SeqCst) {
                return Err(FilesystemError::NotFound);
            }
            self.memory.metadata(path)
        }

        fn read_dir(&self, path: &str) -> Result<Vec<DirEntry>, FilesystemError> {
            self.memory.read_dir(path)
        }

        fn create_dir(&self, path: &str) -> Result<(), FilesystemError> {
            self.memory.create_dir(path)
        }

        fn remove_dir(&self, path: &str) -> Result<(), FilesystemError> {
            self.memory.remove_dir(path)
        }

        fn remove_file(&self, path: &str) -> Result<(), FilesystemError> {
            self.memory.remove_file(path)
        }

        fn rename(&self, from: &str, to: &str, rename: Rename) -> Result<(), FilesystemError> {
            self.memory.rename(from, to, rename)
        }
    }

    /// A Memory storing names in lower case and reaching each by any
    /// case, as a host that folds case does.
    struct FoldsCase(Memory);

    impl Filesystem for FoldsCase {
        fn open(
            &self,
            path: &str,
            options: &OpenOptions,
        ) -> Result<Box<dyn File>, FilesystemError> {
            self.0.open(&path.to_lowercase(), options)
        }

        fn metadata(&self, path: &str) -> Result<Metadata, FilesystemError> {
            self.0.metadata(&path.to_lowercase())
        }

        fn stored_path(&self, path: &str) -> Result<String, FilesystemError> {
            Ok(path.to_lowercase())
        }

        fn read_dir(&self, path: &str) -> Result<Vec<DirEntry>, FilesystemError> {
            self.0.read_dir(&path.to_lowercase())
        }

        fn create_dir(&self, path: &str) -> Result<(), FilesystemError> {
            self.0.create_dir(&path.to_lowercase())
        }

        fn remove_dir(&self, path: &str) -> Result<(), FilesystemError> {
            self.0.remove_dir(&path.to_lowercase())
        }

        fn remove_file(&self, path: &str) -> Result<(), FilesystemError> {
            self.0.remove_file(&path.to_lowercase())
        }

        fn rename(&self, from: &str, to: &str, rename: Rename) -> Result<(), FilesystemError> {
            self.0
                .rename(&from.to_lowercase(), &to.to_lowercase(), rename)
        }
    }

    // @behavior FS-083
    #[test]
    fn an_overlay_spells_a_path_as_the_layer_showing_it_stores_it() {
        let overlay = Overlay::new()
            .layer(Arc::new(Memory::new()), Access::ReadWrite)
            .layer(
                Arc::new(FoldsCase([("private/secret", "x")].into_iter().collect())),
                Access::Read,
            );
        assert_eq!(
            (
                overlay.stored_path("PRIVATE/SECRET"),
                overlay.stored_path("PRIVATE/New")
            ),
            (
                Ok("private/secret".to_string()),
                Ok("private/New".to_string())
            ),
            "a path asked of Overlay::stored_path in another case, shown by a Layer folding case, \
             must come back as that Layer stores each name, a new name kept as given"
        );
    }

    // A whiteout the Layer cannot read would otherwise count as absent and
    // let the lower file show.
    // @behavior FS-079
    #[test]
    fn a_layer_that_cannot_read_its_markers_fails_the_lookup() {
        let top: Memory = [(".wh.file", "")].into_iter().collect();
        let overlay = Overlay::new()
            .layer(Arc::new(ReadsNoMarker(top)), Access::Read)
            .layer(memory_with("file", "hidden"), Access::Read);
        assert!(
            matches!(overlay.metadata("file"), Err(FilesystemError::Other(_))),
            "a file looked up through Overlay::metadata, beneath a Layer that cannot read the \
             whiteout over it, must fail the lookup rather than show the file"
        );
    }

    // @behavior FS-080
    #[test]
    fn a_file_gone_during_a_rename_answers_not_found() {
        let lower = Vanishes {
            memory: [("file", "lower")].into_iter().collect(),
            seen: AtomicBool::new(false),
        };
        let overlay = Overlay::new()
            .layer(Arc::new(Memory::new()), Access::ReadWrite)
            .layer(Arc::new(lower), Access::Read);
        assert_eq!(
            overlay.rename("file", "moved", Rename::Any),
            Err(FilesystemError::NotFound),
            "a file renamed through Overlay::rename that its Layer loses during the call must \
             answer not found"
        );
    }

    // The upper Layer's `dir/sub` stands where the lower Layer holds a
    // file `dir`, so the merge below it meets a file on the way.
    // @behavior FS-078
    #[test]
    fn a_directory_over_a_lower_file_on_its_way_reads_as_a_directory() {
        let overlay = Overlay::new()
            .layer(memory_with("dir/sub/file", "upper"), Access::ReadWrite)
            .layer(memory_with("dir", "lower"), Access::Read);
        assert_eq!(
            (
                overlay.metadata("dir/sub").map(|metadata| metadata.file_type),
                read(&overlay, "dir/sub/file")
            ),
            (Ok(FileType::Directory), b"upper".to_vec()),
            "a directory looked up through Overlay::metadata above a lower Layer holding a file on \
             its path must read as a directory, as its files open"
        );
    }

    // The writable Layer refuses each change only after the Overlay has
    // cleared the whiteout from `dir`, so only putting it back keeps the
    // read-only Layer's `dir/file` hidden.
    // @behavior FS-063
    #[test]
    fn a_refused_change_leaves_what_a_whiteout_hid_hidden() {
        let (overlay, _) = overlay_on(Arc::new(HoldsDirectories(Memory::new())));
        overlay
            .remove_file("dir/file")
            .expect("the file must be removed");
        overlay
            .create_dir("new")
            .expect("the directory must be made");
        let refused = (
            overlay.rename("new", "dir", Rename::Directory),
            overlay.remove_dir("dir"),
        );
        assert_eq!(
            (refused, overlay.metadata("dir/file").err()),
            (
                (
                    Err(FilesystemError::PermissionDenied),
                    Err(FilesystemError::PermissionDenied)
                ),
                Some(FilesystemError::NotFound)
            ),
            "a rename onto and a removal of a directory whose writable copy holds only a whiteout, \
             refused by the Layer through an Overlay, must leave the whited-out file hidden"
        );
    }

    fn trickles(refuses_close: bool) -> Arc<Trickles> {
        Arc::new(Trickles {
            memory: Memory::new(),
            refuses_close,
        })
    }

    // @behavior FS-054
    #[test]
    fn a_copy_up_through_a_layer_writing_in_parts_copies_the_whole_file() {
        let upper = trickles(false);
        let (overlay, _) = overlay_on(upper.clone());
        overlay
            .open(
                "dir/file",
                &OpenOptions {
                    write: true,
                    ..OpenOptions::default()
                },
            )
            .expect("the open must copy the lower file up");
        assert_eq!(
            read(&upper.memory, "dir/file"),
            b"lower".to_vec(),
            "a write open through Overlay::open must copy the whole lower file up into a Layer \
             whose writes land a few bytes at a time"
        );
    }

    // @behavior FS-055
    #[test]
    fn a_copy_up_whose_close_a_layer_refuses_leaves_nothing_behind() {
        let upper = trickles(true);
        let (overlay, _) = overlay_on(upper.clone());
        assert_eq!(
            (
                write(&overlay, "dir/file", b"x"),
                upper.metadata("dir/file"),
                read(&overlay, "dir/file")
            ),
            (
                Err(FilesystemError::PermissionDenied),
                Err(FilesystemError::NotFound),
                b"lower".to_vec()
            ),
            "a write through Overlay::open whose copy up the writable Layer refuses to close must \
             leave no copy hiding the lower file"
        );
    }

    /// Counts each wait its Overlay's lock makes through it.
    struct CountsWaits(AtomicUsize);

    impl Blocking for CountsWaits {
        fn wait(&self, wait: &mut dyn FnMut()) {
            self.0.fetch_add(1, Ordering::SeqCst);
            wait();
        }
    }

    // @behavior FS-039
    #[test]
    fn a_contended_overlay_waits_through_its_blocking() {
        let counts = Arc::new(CountsWaits(AtomicUsize::new(0)));
        let overlay = Overlay::new()
            .layer(memory_with("note", "x"), Access::Read)
            .with_blocking(counts.clone());
        let held = overlay.lock.hold().expect("the lock must be free");
        let other = overlay.clone();
        let waiter = thread::spawn(move || other.metadata("note").map(|m| m.file_type));
        while counts.0.load(Ordering::SeqCst) == 0 {
            thread::sleep(Duration::from_millis(1));
        }
        drop(held);
        assert_eq!(
            (
                waiter.join().expect("the waiter must not panic"),
                counts.0.load(Ordering::SeqCst)
            ),
            (Ok(FileType::File), 1),
            "a call through an Overlay whose lock another thread holds must wait through the \
             Blocking given by Overlay::with_blocking, then answer"
        );
    }

    // Copying up makes the directory in the writable Layer, whose own
    // identity differs, so only reporting the lower Layer's keeps it.
    // @behavior FS-072
    #[test]
    fn an_overlay_keeps_a_directorys_identity_when_a_write_copies_it_up() {
        let lower: Arc<dyn Filesystem> =
            Arc::new([("dir/a.txt", "a")].into_iter().collect::<Memory>());
        let overlay = Overlay::new()
            .layer(Arc::new(Memory::new()), Access::ReadWrite)
            .layer(lower, Access::Read);
        let identity = |overlay: &Overlay| {
            let metadata = overlay.metadata("dir").expect("the directory must stand");
            (metadata.dev, metadata.ino)
        };
        let before = identity(&overlay);
        overwrite(&overlay, "dir/b.txt", b"b");
        assert_eq!(
            (identity(&overlay) == before, before.1 != 0),
            (true, true),
            "a directory of a read-only Layer copied up by a write through an Overlay's open must \
             keep the identity Overlay::metadata reported before"
        );
    }

    // @behavior FS-073
    #[test]
    fn an_overlay_names_a_directory_alike_whether_a_link_at_its_end_is_followed() {
        let layer = |name: &str| -> Arc<dyn Filesystem> {
            Arc::new(
                [(format!("dir/{name}"), "x")]
                    .into_iter()
                    .collect::<Memory>(),
            )
        };
        let overlay = Overlay::new()
            .layer(layer("upper"), Access::ReadWrite)
            .layer(layer("lower"), Access::Read);
        let identity = |metadata: Result<Metadata, FilesystemError>| {
            let metadata = metadata.expect("the directory must stand");
            (metadata.dev, metadata.ino)
        };
        assert_eq!(
            identity(overlay.symlink_metadata("dir")),
            identity(overlay.metadata("dir")),
            "a directory two Layers hold, looked up through Overlay::symlink_metadata, must \
             report the identity Overlay::metadata does"
        );
    }
}
