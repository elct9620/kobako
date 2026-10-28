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
mod lower;
mod merged;
mod readers;
mod stack;

use std::any::Any;
use std::sync::Arc;

pub use exclusive::Blocking;

use super::path::components;
use super::{Access, DirEntry, File, FilesystemError, Inode, Metadata, OpenOptions, Rename};
use crate::vfs::{Dentry, Filesystem};
use exclusive::{Exclusive, Inline};
use merged::MergedDir;
use stack::{Chain, Stack};

/// Layers stacked into one Filesystem, the first one added on top. Its
/// clones share its lock; two Overlays sharing a writable Layer are not
/// serialized against each other, as overlayfs leaves two mounts sharing
/// an upper directory undefined.
///
/// A directory opened beneath it is an Overlay too, holding each Layer's
/// directory it stands for, so it is reached wherever those directories
/// move, as a descriptor reaches its directory. overlayfs leaves a change
/// made to a Layer outside it undefined; here such a change is the
/// host's, and a directory opened before it keeps the Layers' directories
/// it holds.
#[derive(Clone)]
pub struct Overlay {
    stack: Stack,
    lock: Arc<Exclusive>,
    /// The directory this one stands for, `None` for the root.
    merged: Option<Arc<MergedDir>>,
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
            merged: None,
        }
    }

    /// The chain to this directory: its Layers' directories as it holds
    /// them, or, for a change through a directory some writable Layer
    /// does not hold yet, the chain from the root, so the change can make
    /// it there first. The caller holds the Overlay's lock.
    fn chain(&self, changes: bool) -> Result<Chain, FilesystemError> {
        let Some(merged) = &self.merged else {
            return self.stack.root();
        };
        let level = merged.level();
        if changes && self.stack.misses_writable(&level) {
            let mut chain = self.stack.root()?;
            self.stack.descend(&mut chain, components(merged.path()))?;
            return Ok(chain);
        }
        Ok(Chain::at(merged.path().to_string(), level))
    }

    /// The directory `name` in this one, as an Overlay of its own. The
    /// caller holds the Overlay's lock.
    fn beneath(&self, name: &str) -> Result<Arc<dyn Inode>, FilesystemError> {
        let mut chain = self.chain(false)?;
        self.stack.enter(&mut chain, name)?;
        let level = chain.level().clone();
        let shared = self.stack.misses_writable(&level);
        let merged = self.stack.merged_dirs().at(chain.path(), level, shared);
        Ok(Arc::new(Self {
            merged: Some(merged),
            ..self.clone()
        }))
    }

    /// This Overlay with `filesystem` as a new Layer beneath the ones
    /// already added, allowing `access` there. The Layer is asked through
    /// that Filesystem's dentries, as overlayfs asks each layer by its own
    /// lookup. It takes a lock of its own, so an Overlay holding the one it
    /// was built from as a Layer does not wait on itself.
    pub fn layer(mut self, filesystem: Arc<Filesystem>, access: Access) -> Self {
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

impl Inode for Overlay {
    fn open(
        &self,
        dentry: &Dentry,
        options: &OpenOptions,
    ) -> Result<Box<dyn File>, FilesystemError> {
        let _held = self.lock.hold()?;
        let changes = options.write || options.truncate || options.create || options.create_new;
        let mut chain = self.chain(changes)?;
        self.stack.open(&mut chain, &dentry.name(), options)
    }

    fn open_dir(&self, dentry: &Dentry) -> Result<Arc<dyn Inode>, FilesystemError> {
        let _held = self.lock.hold()?;
        self.beneath(&dentry.name())
    }

    fn metadata(&self) -> Result<Metadata, FilesystemError> {
        let _held = self.lock.hold()?;
        self.stack.dir_metadata(self.chain(false)?.level())
    }

    fn symlink_metadata(&self, dentry: &Dentry) -> Result<Metadata, FilesystemError> {
        let _held = self.lock.hold()?;
        self.stack
            .symlink_metadata(self.chain(false)?.level(), &dentry.name())
    }

    fn stored_name(&self, dentry: &Dentry) -> Result<String, FilesystemError> {
        let _held = self.lock.hold()?;
        self.stack
            .stored_name(self.chain(false)?.level(), &dentry.name())
    }

    fn read_link(&self, dentry: &Dentry) -> Result<String, FilesystemError> {
        let _held = self.lock.hold()?;
        self.stack
            .read_link(self.chain(false)?.level(), &dentry.name())
    }

    fn entries(&self) -> Result<Vec<DirEntry>, FilesystemError> {
        let _held = self.lock.hold()?;
        self.stack.entries(self.chain(false)?.level())
    }

    fn create_dir(&self, dentry: &Dentry) -> Result<(), FilesystemError> {
        let _held = self.lock.hold()?;
        self.stack
            .create_dir(&mut self.chain(true)?, &dentry.name())
    }

    fn remove_dir(&self, dentry: &Dentry) -> Result<(), FilesystemError> {
        let _held = self.lock.hold()?;
        let (mut chain, name) = (self.chain(true)?, dentry.name());
        self.stack.remove_dir(&mut chain, &name)?;
        self.stack.merged_dirs().forget(&chain.path_of(&name));
        Ok(())
    }

    fn remove_file(&self, dentry: &Dentry) -> Result<(), FilesystemError> {
        let _held = self.lock.hold()?;
        self.stack
            .remove_file(&mut self.chain(true)?, &dentry.name())
    }

    /// Both directories lie in one Overlay, which holds one lock however
    /// many directories of it are open. A directory replaced at `to` is
    /// no handle's that a directory made there later reaches.
    fn rename(
        &self,
        from: &Dentry,
        to_dir: &dyn Inode,
        to: &Dentry,
        rename: Rename,
    ) -> Result<(), FilesystemError> {
        let target = (to_dir as &dyn Any)
            .downcast_ref::<Self>()
            .filter(|target| Arc::ptr_eq(&target.lock, &self.lock))
            .ok_or(FilesystemError::CrossesDevices)?;
        let _held = self.lock.hold()?;
        let (mut from_chain, from) = (self.chain(true)?, from.name());
        let (mut to_chain, to) = (target.chain(true)?, to.name());
        self.stack
            .rename(&mut from_chain, &from, &mut to_chain, &to, rename)?;
        if from_chain.path_of(&from) != to_chain.path_of(&to) {
            self.stack.merged_dirs().forget(&to_chain.path_of(&to));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::Mutex;
    use std::thread;
    use std::time::{Duration, Instant};

    use super::super::tmpfs::Tmpfs;
    use super::super::FileType;
    use super::*;
    use crate::vfs::kern_path::{kern_path, KernPath};

    /// The answer of a test Filesystem holding no links.
    macro_rules! holds_no_links {
        () => {
            fn read_link(&self, dentry: &Dentry) -> Result<String, FilesystemError> {
                self.symlink_metadata(dentry)?;
                Err(FilesystemError::InvalidInput)
            }
        };
    }

    /// The answer of a test Filesystem matching names byte for byte.
    macro_rules! matches_names_exactly {
        () => {
            fn stored_name(&self, dentry: &Dentry) -> Result<String, FilesystemError> {
                Ok(dentry.name())
            }
        };
    }

    /// The answers of a test Filesystem over the Tmpfs in `$inner` that
    /// opens, lists, and reports its directories as that Tmpfs does.
    macro_rules! opens_directories {
        ($inner:tt) => {
            fn open_dir(&self, dentry: &Dentry) -> Result<Arc<dyn Inode>, FilesystemError> {
                self.$inner.open_dir(dentry)
            }

            fn metadata(&self) -> Result<Metadata, FilesystemError> {
                self.$inner.metadata()
            }

            fn entries(&self) -> Result<Vec<DirEntry>, FilesystemError> {
                self.$inner.entries()
            }
        };
    }

    /// `filesystem` reached through the VFS.
    fn at(filesystem: impl Inode) -> KernPath {
        kern_path(Arc::new(filesystem))
    }

    /// The Filesystem `filesystem` holds, reached through the VFS.
    fn on<F: Inode>(filesystem: &Arc<F>) -> KernPath {
        kern_path(filesystem.clone())
    }

    fn open_to_read(fs: &KernPath, path: &str) -> Box<dyn File> {
        fs.open(
            path,
            &OpenOptions {
                read: true,
                ..OpenOptions::default()
            },
        )
        .expect("the file must open to read")
    }

    fn read_from(file: &mut dyn File) -> Vec<u8> {
        let mut buf = vec![0; 64];
        let read = file.read_at(&mut buf, 0).expect("the file must read");
        buf.truncate(read);
        buf
    }

    fn read(fs: &KernPath, path: &str) -> Vec<u8> {
        read_from(open_to_read(fs, path).as_mut())
    }

    fn write(fs: &KernPath, path: &str, contents: &[u8]) -> Result<(), FilesystemError> {
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

    fn overwrite(fs: &KernPath, path: &str, contents: &[u8]) {
        write(fs, path, contents).expect("the write must land");
    }

    fn names(fs: &KernPath, path: &str) -> Vec<String> {
        let listed = match path {
            "" => fs.entries(),
            path => fs.open_dir(path).and_then(|dir| dir.entries()),
        };
        let mut names: Vec<String> = listed
            .expect("the directory must list")
            .into_iter()
            .map(|entry| entry.name)
            .collect();
        names.sort();
        names
    }

    fn tmpfs_with(path: &str, contents: &str) -> Arc<Tmpfs> {
        Arc::new([(path, contents)].into_iter().collect())
    }

    /// A writable `upper` over a read-only Tmpfs holding `dir/file` =
    /// "lower".
    fn overlay_on(upper: Arc<dyn Inode>) -> (KernPath, Arc<Tmpfs>) {
        let lower = tmpfs_with("dir/file", "lower");
        let overlay = Overlay::new()
            .layer(Filesystem::new(upper), Access::ReadWrite)
            .layer(Filesystem::new(lower.clone()), Access::Read);
        (at(overlay), lower)
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

        /// A writable Tmpfs over the host directory as a read-only Layer.
        fn overlay(&self) -> KernPath {
            self.under(Arc::new(Tmpfs::new()))
        }

        /// The host directory as a read-only Layer beneath `upper`.
        fn under(&self, upper: Arc<dyn Inode>) -> KernPath {
            let lower = super::super::Bind::new(&self.0).expect("the host directory must open");
            at(Overlay::new()
                .layer(Filesystem::new(upper), Access::ReadWrite)
                .layer(Filesystem::new(Arc::new(lower)), Access::Read))
        }
    }

    // The upper Layer's `dirlink` is a directory and the lower one's a link,
    // so only merging directory over directory keeps the lower link's
    // target out of the view.
    // @behavior FS-113
    #[cfg(unix)]
    #[test]
    fn a_lower_layers_link_beneath_an_upper_directory_is_not_merged() {
        let tree = LinkTree::new("not-merged");
        let upper = Arc::new(Tmpfs::new());
        on(&upper)
            .create_dir("dirlink")
            .expect("the upper directory must be made");
        let overlay = tree.under(upper);
        assert_eq!(
            (
                kind(&overlay, "dirlink/inner"),
                names(&overlay, "dirlink").len()
            ),
            (Err(FilesystemError::NotFound), 0),
            "a name beneath a directory an upper Layer holds, where a lower Layer holds a link, \
             through Overlay::metadata and its listing must show nothing the link leads to"
        );
    }

    #[cfg(unix)]
    impl Drop for LinkTree {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    // The link stands in the Bind Layer beneath a Tmpfs one, so only
    // the Overlay asking that Layer without following refuses it.
    // @behavior FS-100
    #[cfg(unix)]
    #[test]
    fn an_overlay_opens_no_link_as_a_directory_where_asked_not_to_follow() {
        let host = LinkTree::new("nofollow");
        let overlay = host.overlay();
        assert_eq!(
            (
                overlay.open_dir("dirlink").map(drop),
                overlay.read_link("dirlink")
            ),
            (Err(FilesystemError::FilesystemLoop), Ok("dir".to_string())),
            "a link a Bind Layer holds, opened through Overlay::open_dir, must be a \
             link loop, and through Overlay::read_link must answer where it points"
        );
    }

    // @behavior FS-049
    #[cfg(unix)]
    #[test]
    fn an_overlay_reports_a_link_its_layer_holds_as_a_link() {
        let host = LinkTree::new("lstat");
        let overlay = host.overlay();
        let file_type = |path| kind(&overlay, path);
        assert_eq!(
            (file_type("link"), file_type("dangling")),
            (Ok(FileType::Symlink), Ok(FileType::Symlink)),
            "a link a Bind Layer holds, pointing at a file or at nothing, looked up through \
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
            "a link a read-only Bind Layer holds, removed through Overlay::remove_dir and \
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
                overlay.rename("pipe", &overlay, "moved", Rename::File)
            ),
            (
                Err(FilesystemError::PermissionDenied),
                Err(FilesystemError::CrossesDevices)
            ),
            "a FIFO a read-only Bind Layer holds, written or renamed through an Overlay, must \
             be refused as permission denied and as crossing devices"
        );
    }

    // A FIFO is no directory, so each rename meets a kind it cannot
    // replace; were it taken for neither, the directory made above would
    // hide the FIFO, and the FIFO's rename would answer for its Layer.
    // @behavior FS-095
    #[cfg(unix)]
    #[test]
    fn an_overlay_replaces_neither_a_fifo_nor_a_directory_with_the_other() {
        let host = LinkTree::new("fifo-kinds");
        let made = std::process::Command::new("mkfifo")
            .arg(host.0.join("pipe"))
            .status()
            .expect("mkfifo must run");
        assert!(made.success(), "the FIFO must be made");
        let overlay = host.overlay();
        overlay
            .create_dir("made")
            .expect("a directory must be made in the writable Layer");
        let file_type = |path| kind(&overlay, path);
        assert_eq!(
            (
                overlay.rename("pipe", &overlay, "dir", Rename::Any),
                overlay.rename("made", &overlay, "pipe", Rename::Any),
                file_type("pipe"),
                file_type("made"),
            ),
            (
                Err(FilesystemError::IsADirectory),
                Err(FilesystemError::NotADirectory),
                Ok(FileType::Unknown),
                Ok(FileType::Directory)
            ),
            "a FIFO renamed onto a directory through an Overlay must be refused as a directory, \
             and a directory renamed onto a FIFO as no directory, leaving both in place"
        );
    }

    // Each change would land in the writable Tmpfs, which holds no link,
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
            overlay.open("dangling", &creates).map(drop),
            overlay.rename("link", &overlay, "moved", Rename::Any),
        ];
        assert_eq!(
            (refused, read(&overlay, "target")),
            (
                [
                    Err(FilesystemError::CrossesDevices),
                    Err(FilesystemError::CrossesDevices),
                    Err(FilesystemError::CrossesDevices)
                ],
                b"lower".to_vec()
            ),
            "a write, a create, and a rename reaching through a link a read-only Bind Layer \
             holds, through an Overlay, must be refused as crossing devices and leave the target \
             as it was"
        );
    }

    // The lower Tmpfs holds a readable `dangling`, so only the upper
    // link standing at that path can keep the read from reaching it.
    // @behavior FS-053
    #[cfg(unix)]
    #[test]
    fn a_link_a_layer_holds_hides_what_the_layers_below_hold_there() {
        let host = LinkTree::new("shadow");
        let upper = super::super::Bind::new(&host.0).expect("the host directory must open");
        let overlay = at(Overlay::new()
            .layer(Filesystem::new(Arc::new(upper)), Access::ReadWrite)
            .layer(
                Filesystem::new(tmpfs_with("dangling", "lower")),
                Access::Read,
            ));
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
            "a lower file beneath a dangling link a higher Bind Layer holds at its path, \
             opened through Overlay::open, must not be found"
        );
    }

    // The lower Tmpfs holds `dirlink/x` beside where the upper link points,
    // so only the link hiding everything beneath it keeps `x` out.
    // @behavior FS-057
    #[cfg(unix)]
    #[test]
    fn a_link_a_layer_holds_hides_what_the_layers_below_hold_beneath_it() {
        let host = LinkTree::new("shadow-beneath");
        let upper = super::super::Bind::new(&host.0).expect("the host directory must open");
        let overlay = at(Overlay::new()
            .layer(Filesystem::new(Arc::new(upper)), Access::ReadWrite)
            .layer(
                Filesystem::new(tmpfs_with("dirlink/x", "lower")),
                Access::Read,
            ));
        assert_eq!(
            (
                overlay
                    .symlink_metadata("dirlink")
                    .map(|metadata| metadata.file_type),
                overlay.open_dir("dirlink").map(drop)
            ),
            (Ok(FileType::Symlink), Err(FilesystemError::FilesystemLoop)),
            "a directory a higher Bind Layer holds as a link, looked up and opened through an \
             Overlay, must stand as the link, so none of a lower Layer's entries beneath it show"
        );
    }

    // The lower directory sits in a writable Layer, so only the whiteout,
    // not a removal that cannot take a directory, keeps the view whole.
    // @behavior FS-058
    #[test]
    fn removing_a_file_over_a_lower_directory_leaves_it_hidden() {
        let lower = tmpfs_with("f/g", "lower");
        let overlay = at(Overlay::new()
            .layer(Filesystem::new(tmpfs_with("f", "upper")), Access::ReadWrite)
            .layer(Filesystem::new(lower.clone()), Access::ReadWrite));
        let removed = overlay.remove_file("f");
        assert_eq!(
            (
                removed,
                overlay.symlink_metadata("f").err(),
                on(&lower).symlink_metadata("f/g").is_ok()
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
        let (overlay, _) = overlay_on(Arc::new(Tmpfs::new()));
        overlay
            .remove_file("dir/file")
            .expect("the file must be removed");
        overlay
            .create_dir("new")
            .expect("the directory must be made");
        overwrite(&overlay, "new/made", b"made");
        let moved = overlay.rename("new", &overlay, "dir", Rename::Directory);
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
        let (overlay, _) = overlay_on(Arc::new(Tmpfs::new()));
        overlay
            .remove_file("dir/file")
            .expect("the file must be removed");
        overlay
            .remove_dir("dir")
            .expect("the directory must be removed");
        overlay
            .create_dir("dir")
            .expect("the directory must be made again");
        let moved = overlay.rename("dir", &overlay, "moved", Rename::Directory);
        assert_eq!(
            (
                moved,
                overlay.symlink_metadata("dir").err(),
                overlay
                    .open_dir("moved")
                    .and_then(|dir| dir.entries())
                    .map(|entries| entries.len())
            ),
            (Ok(()), Some(FilesystemError::NotFound), Ok(0)),
            "a directory made again through an Overlay where a read-only Layer's one was removed, \
             renamed through Overlay::rename, must move and leave nothing of the lower one behind"
        );
    }

    // @behavior FS-061
    #[test]
    fn an_overlay_refuses_a_file_moved_beneath_itself_as_no_directory() {
        let (overlay, _) = overlay_on(Arc::new(Tmpfs::new()));
        assert_eq!(
            overlay.rename("dir/file", &overlay, "dir/file/x", Rename::Any),
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
            "an exclusive create and a create_dir at a dangling link a read-only Bind Layer \
             holds, through an Overlay, must find it already there"
        );
    }

    // @behavior FS-008
    #[test]
    fn an_overlay_reads_through_to_a_lower_layer() {
        let (overlay, _) = overlay_on(Arc::new(Tmpfs::new()));
        assert_eq!(
            read(&overlay, "dir/file"),
            b"lower",
            "a lower file read through Overlay::open must read back the lower content"
        );
    }

    // @behavior FS-009
    #[test]
    fn a_write_to_a_read_only_layers_file_lands_in_the_nearest_writable_layer_above_it() {
        let upper = Arc::new(Tmpfs::new());
        let (overlay, _) = overlay_on(upper.clone());
        overwrite(&overlay, "dir/file", b"upper");
        assert_eq!(
            read(&on(&upper), "dir/file"),
            b"upper",
            "a read-only Layer's file written through Overlay::open must land in the writable Layer above it"
        );
    }

    // @behavior FS-010
    #[test]
    fn an_overlay_leaves_a_read_only_layer_as_it_was() {
        let (overlay, lower) = overlay_on(Arc::new(Tmpfs::new()));
        overwrite(&overlay, "dir/file", b"upper");
        overlay
            .remove_file("dir/file")
            .expect("the file must be removed");
        assert_eq!(
            read(&on(&lower), "dir/file"),
            b"lower",
            "a read-only Layer's file written and removed through an Overlay must stay as it was"
        );
    }

    // @behavior FS-011
    #[test]
    fn a_file_removed_through_an_overlay_is_gone_from_its_view() {
        let (overlay, _) = overlay_on(Arc::new(Tmpfs::new()));
        overlay
            .remove_file("dir/file")
            .expect("the file must be removed");
        assert_eq!(
            overlay.symlink_metadata("dir/file"),
            Err(FilesystemError::NotFound),
            "a lower file removed through Overlay::remove_file must no longer be found there"
        );
    }

    // @behavior FS-013
    #[test]
    fn an_upper_layer_hides_the_same_path_in_a_lower_one() {
        let overlay = at(Overlay::new()
            .layer(Filesystem::new(tmpfs_with("note", "upper")), Access::Read)
            .layer(Filesystem::new(tmpfs_with("note", "lower")), Access::Read));
        assert_eq!(
            read(&overlay, "note"),
            b"upper",
            "a path both Layers hold read through Overlay::open must read the upper Layer's content"
        );
    }

    // @behavior FS-014
    #[test]
    fn a_write_to_a_writable_layers_file_changes_it_in_place() {
        let upper = Arc::new(Tmpfs::new());
        let lower = tmpfs_with("note", "lower");
        let overlay = at(Overlay::new()
            .layer(Filesystem::new(upper.clone()), Access::ReadWrite)
            .layer(Filesystem::new(lower.clone()), Access::ReadWrite));
        overwrite(&overlay, "note", b"changed");
        assert_eq!(
            (read(&on(&lower), "note"), on(&upper).symlink_metadata("note")),
            (b"changed".to_vec(), Err(FilesystemError::NotFound)),
            "a writable lower Layer's file written through Overlay::open must change in that Layer alone"
        );
    }

    // The lower Layer is writable, so only the refusal itself keeps the
    // move from landing in two Layers.
    // @behavior FS-088
    #[test]
    fn an_overlay_refuses_to_move_a_file_a_lower_writable_layer_holds() {
        let upper = Arc::new(Tmpfs::new());
        let lower = tmpfs_with("note", "lower");
        let overlay = at(Overlay::new()
            .layer(Filesystem::new(upper.clone()), Access::ReadWrite)
            .layer(Filesystem::new(lower.clone()), Access::ReadWrite));
        assert_eq!(
            (
                overlay.rename("note", &overlay, "moved", Rename::File),
                read(&on(&lower), "note"),
                on(&upper).entries().map(|entries| entries.len())
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
        let (overlay, _) = overlay_on(Arc::new(KeepsWhiteouts(Tmpfs::new())));
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
        let first = Arc::new(Tmpfs::new());
        let second = Arc::new(Tmpfs::new());
        let overlay = at(Overlay::new()
            .layer(Filesystem::new(Arc::new(Tmpfs::new())), Access::Read)
            .layer(Filesystem::new(first.clone()), Access::ReadWrite)
            .layer(Filesystem::new(second.clone()), Access::ReadWrite));
        overwrite(&overlay, "new", b"made");
        assert_eq!(
            (
                read(&on(&first), "new"),
                on(&second).symlink_metadata("new")
            ),
            (b"made".to_vec(), Err(FilesystemError::NotFound)),
            "a file created through Overlay::open must land in the topmost writable Layer"
        );
    }

    // @behavior FS-016
    #[test]
    fn a_removal_leaves_a_whiteout_in_the_topmost_writable_layer() {
        let upper = Arc::new(Tmpfs::new());
        let (overlay, _) = overlay_on(upper.clone());
        overlay
            .remove_file("dir/file")
            .expect("the file must be removed");
        assert_eq!(
            on(&upper).symlink_metadata("dir/.wh.file").map(|metadata| metadata.file_type),
            Ok(FileType::File),
            "a read-only Layer's file removed through Overlay::remove_file must leave .wh.file beside it in the writable Layer"
        );
    }

    // @behavior FS-017
    #[test]
    fn a_whiteout_lasts_as_long_as_its_writable_layer() {
        let upper = Arc::new(Tmpfs::new());
        let (overlay, lower) = overlay_on(upper.clone());
        overlay
            .remove_file("dir/file")
            .expect("the file must be removed");
        let rebuilt = at(Overlay::new()
            .layer(Filesystem::new(upper), Access::ReadWrite)
            .layer(Filesystem::new(lower), Access::Read));
        assert_eq!(
            rebuilt.symlink_metadata("dir/file"),
            Err(FilesystemError::NotFound),
            "a lower file removed through an Overlay over a Tmpfs must stay gone from a new Overlay over the same Layers"
        );
    }

    // @behavior FS-018
    #[test]
    fn a_directory_made_again_where_one_was_removed_shows_nothing_from_below() {
        let (overlay, _) = overlay_on(Arc::new(Tmpfs::new()));
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
        let (overlay, _) = overlay_on(Arc::new(Tmpfs::new()));
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
        let (overlay, _) = overlay_on(Arc::new(Tmpfs::new()));
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
        let (overlay, _) = overlay_on(Arc::new(Tmpfs::new()));
        assert_eq!(
            write(&overlay, "dir/.WH.note", b"x"),
            Err(FilesystemError::InvalidInput),
            "a file named .WH.note created through Overlay::open must be refused"
        );
    }

    // @behavior FS-021
    #[test]
    fn an_overlay_with_no_writable_layer_refuses_every_change_as_read_only() {
        let overlay = at(Overlay::new().layer(
            Filesystem::new(tmpfs_with("dir/file", "lower")),
            Access::Read,
        ));
        assert_eq!(
            write(&overlay, "dir/file", b"x"),
            Err(FilesystemError::ReadOnlyFilesystem),
            "a file written through an Overlay of read-only Layers must be refused as read-only"
        );
    }

    #[test]
    fn an_overlay_lists_every_layer_without_what_it_hid() {
        let (overlay, _) = overlay_on(Arc::new(Tmpfs::new()));
        overwrite(&overlay, "dir/new", b"made");
        assert_eq!(names(&overlay, "dir"), ["file", "new"]);
    }

    #[test]
    fn a_lower_file_renamed_through_an_overlay_moves_into_the_writable_layer() {
        let upper = Arc::new(Tmpfs::new());
        let (overlay, lower) = overlay_on(upper.clone());
        overlay
            .rename("dir/file", &overlay, "dir/moved", Rename::Any)
            .expect("the file must move");
        assert_eq!(
            (
                read(&overlay, "dir/moved"),
                overlay.symlink_metadata("dir/file"),
                read(&on(&lower), "dir/file"),
            ),
            (
                b"lower".to_vec(),
                Err(FilesystemError::NotFound),
                b"lower".to_vec()
            ),
        );
    }

    /// A Tmpfs that, the first time it opens a directory, starts another
    /// call through `overlay` on a thread of its own and gives it a while
    /// to finish, recording whether it did before this call returned.
    struct Interleaves {
        tmpfs: Arc<Tmpfs>,
        overlay: Mutex<Option<KernPath>>,
        finished_inside: Mutex<Option<bool>>,
        other: Mutex<Option<thread::JoinHandle<()>>>,
    }

    impl Inode for Interleaves {
        holds_no_links!();
        matches_names_exactly!();

        fn metadata(&self) -> Result<Metadata, FilesystemError> {
            self.tmpfs.metadata()
        }

        fn open(
            &self,
            dentry: &Dentry,
            options: &OpenOptions,
        ) -> Result<Box<dyn File>, FilesystemError> {
            self.tmpfs.open(dentry, options)
        }

        fn symlink_metadata(&self, dentry: &Dentry) -> Result<Metadata, FilesystemError> {
            self.tmpfs.symlink_metadata(dentry)
        }

        fn open_dir(&self, dentry: &Dentry) -> Result<Arc<dyn Inode>, FilesystemError> {
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
            self.tmpfs.open_dir(dentry)
        }

        fn entries(&self) -> Result<Vec<DirEntry>, FilesystemError> {
            self.tmpfs.entries()
        }

        fn create_dir(&self, dentry: &Dentry) -> Result<(), FilesystemError> {
            self.tmpfs.create_dir(dentry)
        }

        fn remove_dir(&self, dentry: &Dentry) -> Result<(), FilesystemError> {
            self.tmpfs.remove_dir(dentry)
        }

        fn remove_file(&self, dentry: &Dentry) -> Result<(), FilesystemError> {
            self.tmpfs.remove_file(dentry)
        }

        fn rename(
            &self,
            from: &Dentry,
            _to_dir: &dyn Inode,
            to: &Dentry,
            rename: Rename,
        ) -> Result<(), FilesystemError> {
            self.tmpfs.rename(from, self.tmpfs.as_ref(), to, rename)
        }
    }

    // The other call starts inside this one, between its emptiness check
    // and its removal, so only the Overlay's lock keeps the file it
    // creates from being removed with the directory.
    // @behavior FS-037
    #[test]
    fn a_change_through_an_overlay_waits_for_the_one_in_progress() {
        let upper = Arc::new(Interleaves {
            tmpfs: tmpfs_with("dir/.keep", ""),
            overlay: Mutex::new(None),
            finished_inside: Mutex::new(None),
            other: Mutex::new(None),
        });
        on(&upper.tmpfs)
            .remove_file("dir/.keep")
            .expect("the seed must go");
        let overlay = at(Overlay::new().layer(Filesystem::new(upper.clone()), Access::ReadWrite));
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
            (removed, *upper.finished_inside.lock().unwrap(), overlay.symlink_metadata("dir/new").is_ok()),
            (Ok(()), Some(false), false),
            "a change through Overlay::open started while Overlay::remove_dir runs must wait for it, \
             failing rather than landing in the directory being removed"
        );
    }

    // @behavior FS-038
    #[test]
    fn a_call_through_an_overlay_from_inside_one_answers_a_deadlock() {
        struct Reenters {
            tmpfs: Tmpfs,
            overlay: Mutex<Option<KernPath>>,
            answer: Mutex<Option<Result<Metadata, FilesystemError>>>,
        }

        impl Inode for Reenters {
            holds_no_links!();
            matches_names_exactly!();

            fn open(
                &self,
                dentry: &Dentry,
                options: &OpenOptions,
            ) -> Result<Box<dyn File>, FilesystemError> {
                self.tmpfs.open(dentry, options)
            }

            fn symlink_metadata(&self, dentry: &Dentry) -> Result<Metadata, FilesystemError> {
                if let Some(overlay) = self.overlay.lock().unwrap().take() {
                    *self.answer.lock().unwrap() = Some(overlay.symlink_metadata(&dentry.name()));
                }
                self.tmpfs.symlink_metadata(dentry)
            }

            opens_directories!(tmpfs);

            fn create_dir(&self, dentry: &Dentry) -> Result<(), FilesystemError> {
                self.tmpfs.create_dir(dentry)
            }

            fn remove_dir(&self, dentry: &Dentry) -> Result<(), FilesystemError> {
                self.tmpfs.remove_dir(dentry)
            }

            fn remove_file(&self, dentry: &Dentry) -> Result<(), FilesystemError> {
                self.tmpfs.remove_file(dentry)
            }

            fn rename(
                &self,
                from: &Dentry,
                _to_dir: &dyn Inode,
                to: &Dentry,
                rename: Rename,
            ) -> Result<(), FilesystemError> {
                self.tmpfs.rename(from, &self.tmpfs, to, rename)
            }
        }

        let layer = Arc::new(Reenters {
            tmpfs: [("note", "x")].into_iter().collect(),
            overlay: Mutex::new(None),
            answer: Mutex::new(None),
        });
        let overlay = at(Overlay::new().layer(Filesystem::new(layer.clone()), Access::Read));
        *layer.overlay.lock().unwrap() = Some(overlay.clone());
        overlay
            .symlink_metadata("note")
            .expect("the outer call must answer");
        assert_eq!(
            layer.answer.lock().unwrap().take(),
            Some(Err(FilesystemError::Deadlock)),
            "a call through an Overlay made from inside one of its own calls on the same thread \
             must answer Deadlock rather than wait forever"
        );
    }

    /// A writable Tmpfs seeded with `files` and room for `inodes`, over a
    /// read-only one holding `dir/file` = "lower".
    fn full_over_lower(files: &[(&str, &str)], inodes: usize) -> (KernPath, Arc<Tmpfs>) {
        let upper = Tmpfs::from_iter(files.iter().copied()).with_nr_inodes(Some(inodes));
        let upper = Arc::new(upper);
        let overlay = Overlay::new()
            .layer(Filesystem::new(upper.clone()), Access::ReadWrite)
            .layer(
                Filesystem::new(tmpfs_with("dir/file", "lower")),
                Access::Read,
            );
        (at(overlay), upper)
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
                overlay.symlink_metadata("dir"),
                on(&upper).symlink_metadata("dir")
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
                overlay.rename("dir/file", &overlay, "dir/moved", Rename::Any),
                overlay.symlink_metadata("dir/moved"),
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
        let lower = tmpfs_with("dir/file", "lower");
        let overlay = at(Overlay::new()
            .layer(
                Filesystem::new(tmpfs_with("dir/.wh.file", "")),
                Access::ReadWrite,
            )
            .layer(Filesystem::new(lower.clone()), Access::ReadWrite));
        assert_eq!(
            (
                overlay.remove_dir("dir"),
                overlay.symlink_metadata("dir").err(),
                read(&on(&lower), "dir/file")
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
        let upper = Arc::new(Tmpfs::new().with_nr_inodes(Some(2)));
        on(&upper)
            .create_dir("moving")
            .expect("the directory must be made");
        let lower = Arc::new(Tmpfs::new());
        on(&lower)
            .create_dir("dir")
            .expect("the directory must be made");
        let overlay = at(Overlay::new()
            .layer(Filesystem::new(upper.clone()), Access::ReadWrite)
            .layer(Filesystem::new(lower), Access::Read));
        assert_eq!(
            (
                overlay.rename("moving", &overlay, "dir", Rename::Directory),
                on(&upper)
                    .symlink_metadata("moving")
                    .map(|metadata| metadata.file_type),
                on(&upper).symlink_metadata("dir").err()
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
        let overlay = at(Overlay::new()
            .layer(Filesystem::new(tmpfs_with("to", "top")), Access::Read)
            .layer(Filesystem::new(Arc::new(Tmpfs::new())), Access::ReadWrite)
            .layer(Filesystem::new(tmpfs_with("from", "moved")), Access::Read));
        assert_eq!(
            (
                overlay.rename("from", &overlay, "to", Rename::Any),
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
        let upper = Arc::new(Tmpfs::new());
        let overlay = at(Overlay::new()
            .layer(
                Filesystem::new(Arc::new(
                    [(".wh.file", ""), (".wh.dir", "")]
                        .into_iter()
                        .collect::<Tmpfs>(),
                )),
                Access::Read,
            )
            .layer(Filesystem::new(upper.clone()), Access::ReadWrite));
        assert_eq!(
            (
                write(&overlay, "file", b"new"),
                overlay.create_dir("dir"),
                on(&upper).symlink_metadata("file").err(),
                on(&upper).symlink_metadata("dir").err()
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

    // The opaque marker hides the writable Layer's directory from the view,
    // so a file landing there would succeed out of sight.
    // @behavior FS-118
    #[test]
    fn a_file_beneath_a_read_only_layers_opaque_directory_is_not_made() {
        let lower: Arc<Tmpfs> = Arc::new([("dir/a.txt", "a")].into_iter().collect());
        let overlay = at(Overlay::new()
            .layer(
                Filesystem::new(Arc::new(
                    [("dir/.wh..wh..opq", "")].into_iter().collect::<Tmpfs>(),
                )),
                Access::Read,
            )
            .layer(Filesystem::new(lower.clone()), Access::ReadWrite));
        assert_eq!(
            (
                write(&overlay, "dir/new.txt", b"new"),
                on(&lower).symlink_metadata("dir/new.txt").err()
            ),
            (
                Err(FilesystemError::ReadOnlyFilesystem),
                Some(FilesystemError::NotFound)
            ),
            "a file made through an Overlay in a directory a read-only Layer marks opaque, over a \
             writable Layer holding it, must be refused as read-only and not made"
        );
    }

    // @behavior FS-043
    #[test]
    fn a_directory_a_lower_layer_holds_does_not_move_through_an_overlay() {
        let (overlay, _) = overlay_on(Arc::new(Tmpfs::new()));
        assert_eq!(
            (
                overlay.rename("dir", &overlay, "moved", Rename::Any),
                overlay.symlink_metadata("moved")
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
        let upper = Arc::new(Tmpfs::new());
        let (overlay, _) = overlay_on(upper.clone());
        overwrite(&overlay, "note", b"x");
        overlay
            .create_dir("empty")
            .expect("the directory must be made");
        assert_eq!(
            (
                overlay.rename("note", &overlay, "dir", Rename::Any),
                overlay.rename("empty", &overlay, "dir", Rename::Any),
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
        let upper = Arc::new(Tmpfs::new());
        let lower: Arc<Tmpfs> = Arc::new([("dir/.keep", "")].into_iter().collect());
        on(&lower)
            .remove_file("dir/.keep")
            .expect("the seed must go");
        let overlay = at(Overlay::new()
            .layer(Filesystem::new(upper.clone()), Access::ReadWrite)
            .layer(Filesystem::new(lower.clone()), Access::Read));
        overlay
            .create_dir("made")
            .expect("the directory must be made");
        overlay
            .rename("made", &overlay, "dir", Rename::Any)
            .expect("the directory must move over the empty one");
        assert_eq!(
            on(&upper)
                .symlink_metadata("dir/.wh..wh..opq")
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
        let upper = Arc::new(Tmpfs::new().with_size(Some(2)).with_nr_inodes(Some(8)));
        let (overlay, _) = overlay_on(upper.clone());
        assert_eq!(
            (
                write(&overlay, "dir/file", b"x"),
                on(&upper).symlink_metadata("dir/file"),
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

    /// A Tmpfs whose files write at most three bytes per call and, when
    /// `refuses_close`, refuse their close.
    struct Trickles {
        tmpfs: Arc<dyn Inode>,
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

        fn append(&mut self, buf: &[u8]) -> Result<(u64, usize), FilesystemError> {
            self.file.append(&buf[..buf.len().min(3)])
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

    impl Inode for Trickles {
        holds_no_links!();
        matches_names_exactly!();

        fn open(
            &self,
            dentry: &Dentry,
            options: &OpenOptions,
        ) -> Result<Box<dyn File>, FilesystemError> {
            Ok(Box::new(TrickleFile {
                file: self.tmpfs.open(dentry, options)?,
                refuses_close: self.refuses_close,
            }))
        }

        fn symlink_metadata(&self, dentry: &Dentry) -> Result<Metadata, FilesystemError> {
            self.tmpfs.symlink_metadata(dentry)
        }

        /// Each directory it opens trickles too.
        fn open_dir(&self, dentry: &Dentry) -> Result<Arc<dyn Inode>, FilesystemError> {
            Ok(Arc::new(Self {
                tmpfs: self.tmpfs.open_dir(dentry)?,
                refuses_close: self.refuses_close,
            }))
        }

        fn metadata(&self) -> Result<Metadata, FilesystemError> {
            self.tmpfs.metadata()
        }

        fn entries(&self) -> Result<Vec<DirEntry>, FilesystemError> {
            self.tmpfs.entries()
        }

        fn create_dir(&self, dentry: &Dentry) -> Result<(), FilesystemError> {
            self.tmpfs.create_dir(dentry)
        }

        fn remove_dir(&self, dentry: &Dentry) -> Result<(), FilesystemError> {
            self.tmpfs.remove_dir(dentry)
        }

        fn remove_file(&self, dentry: &Dentry) -> Result<(), FilesystemError> {
            self.tmpfs.remove_file(dentry)
        }

        fn rename(
            &self,
            from: &Dentry,
            _to_dir: &dyn Inode,
            to: &Dentry,
            rename: Rename,
        ) -> Result<(), FilesystemError> {
            let to_dir = (_to_dir as &dyn Any)
                .downcast_ref::<Self>()
                .ok_or(FilesystemError::CrossesDevices)?;
            self.tmpfs.rename(from, to_dir.tmpfs.as_ref(), to, rename)
        }
    }

    /// A Tmpfs refusing to move or remove a directory.
    struct HoldsDirectories(Tmpfs);

    impl Inode for HoldsDirectories {
        holds_no_links!();
        matches_names_exactly!();

        fn open(
            &self,
            dentry: &Dentry,
            options: &OpenOptions,
        ) -> Result<Box<dyn File>, FilesystemError> {
            self.0.open(dentry, options)
        }

        fn symlink_metadata(&self, dentry: &Dentry) -> Result<Metadata, FilesystemError> {
            self.0.symlink_metadata(dentry)
        }

        opens_directories!(0);

        fn create_dir(&self, dentry: &Dentry) -> Result<(), FilesystemError> {
            self.0.create_dir(dentry)
        }

        fn remove_dir(&self, _dentry: &Dentry) -> Result<(), FilesystemError> {
            Err(FilesystemError::PermissionDenied)
        }

        fn remove_file(&self, dentry: &Dentry) -> Result<(), FilesystemError> {
            self.0.remove_file(dentry)
        }

        fn rename(
            &self,
            _from: &Dentry,
            _to_dir: &dyn Inode,
            _to: &Dentry,
            _rename: Rename,
        ) -> Result<(), FilesystemError> {
            Err(FilesystemError::PermissionDenied)
        }
    }

    /// A Tmpfs refusing to remove a whiteout.
    struct KeepsWhiteouts(Tmpfs);

    impl Inode for KeepsWhiteouts {
        holds_no_links!();
        matches_names_exactly!();

        fn open(
            &self,
            dentry: &Dentry,
            options: &OpenOptions,
        ) -> Result<Box<dyn File>, FilesystemError> {
            self.0.open(dentry, options)
        }

        fn symlink_metadata(&self, dentry: &Dentry) -> Result<Metadata, FilesystemError> {
            self.0.symlink_metadata(dentry)
        }

        opens_directories!(0);

        fn create_dir(&self, dentry: &Dentry) -> Result<(), FilesystemError> {
            self.0.create_dir(dentry)
        }

        fn remove_dir(&self, dentry: &Dentry) -> Result<(), FilesystemError> {
            self.0.remove_dir(dentry)
        }

        fn remove_file(&self, dentry: &Dentry) -> Result<(), FilesystemError> {
            if dentry.name().starts_with(".wh.") {
                return Err(FilesystemError::PermissionDenied);
            }
            self.0.remove_file(dentry)
        }

        fn rename(
            &self,
            from: &Dentry,
            _to_dir: &dyn Inode,
            to: &Dentry,
            rename: Rename,
        ) -> Result<(), FilesystemError> {
            self.0.rename(from, &self.0, to, rename)
        }
    }

    /// A Tmpfs answering a lookup of any marker name with an I/O failure.
    struct ReadsNoMarker(Tmpfs);

    impl Inode for ReadsNoMarker {
        holds_no_links!();
        matches_names_exactly!();

        fn open(
            &self,
            dentry: &Dentry,
            options: &OpenOptions,
        ) -> Result<Box<dyn File>, FilesystemError> {
            self.0.open(dentry, options)
        }

        fn symlink_metadata(&self, dentry: &Dentry) -> Result<Metadata, FilesystemError> {
            if dentry.name().starts_with(".wh.") {
                return Err(FilesystemError::Other(
                    "the marker cannot be read".to_string(),
                ));
            }
            self.0.symlink_metadata(dentry)
        }

        opens_directories!(0);

        fn create_dir(&self, dentry: &Dentry) -> Result<(), FilesystemError> {
            self.0.create_dir(dentry)
        }

        fn remove_dir(&self, dentry: &Dentry) -> Result<(), FilesystemError> {
            self.0.remove_dir(dentry)
        }

        fn remove_file(&self, dentry: &Dentry) -> Result<(), FilesystemError> {
            self.0.remove_file(dentry)
        }

        fn rename(
            &self,
            from: &Dentry,
            _to_dir: &dyn Inode,
            to: &Dentry,
            rename: Rename,
        ) -> Result<(), FilesystemError> {
            self.0.rename(from, &self.0, to, rename)
        }
    }

    /// A Tmpfs answering a lookup of `dir` with an I/O failure, though it
    /// opens `dir` as any other directory.
    struct LooksUpNoDir(Tmpfs);

    impl Inode for LooksUpNoDir {
        holds_no_links!();
        matches_names_exactly!();

        fn open(
            &self,
            dentry: &Dentry,
            options: &OpenOptions,
        ) -> Result<Box<dyn File>, FilesystemError> {
            self.0.open(dentry, options)
        }

        fn symlink_metadata(&self, dentry: &Dentry) -> Result<Metadata, FilesystemError> {
            if dentry.name() == "dir" {
                return Err(FilesystemError::Other(
                    "dir cannot be looked up".to_string(),
                ));
            }
            self.0.symlink_metadata(dentry)
        }

        opens_directories!(0);

        fn create_dir(&self, dentry: &Dentry) -> Result<(), FilesystemError> {
            self.0.create_dir(dentry)
        }

        fn remove_dir(&self, dentry: &Dentry) -> Result<(), FilesystemError> {
            self.0.remove_dir(dentry)
        }

        fn remove_file(&self, dentry: &Dentry) -> Result<(), FilesystemError> {
            self.0.remove_file(dentry)
        }

        fn rename(
            &self,
            from: &Dentry,
            _to_dir: &dyn Inode,
            to: &Dentry,
            rename: Rename,
        ) -> Result<(), FilesystemError> {
            self.0.rename(from, &self.0, to, rename)
        }
    }

    /// A Tmpfs whose `file` is gone after its first lookup, as a host
    /// directory another process changes mid-call.
    struct Vanishes {
        tmpfs: Tmpfs,
        seen: AtomicBool,
    }

    impl Inode for Vanishes {
        holds_no_links!();
        matches_names_exactly!();

        fn open(
            &self,
            dentry: &Dentry,
            options: &OpenOptions,
        ) -> Result<Box<dyn File>, FilesystemError> {
            self.tmpfs.open(dentry, options)
        }

        fn symlink_metadata(&self, dentry: &Dentry) -> Result<Metadata, FilesystemError> {
            if dentry.name() == "file" && self.seen.swap(true, Ordering::SeqCst) {
                return Err(FilesystemError::NotFound);
            }
            self.tmpfs.symlink_metadata(dentry)
        }

        opens_directories!(tmpfs);

        fn create_dir(&self, dentry: &Dentry) -> Result<(), FilesystemError> {
            self.tmpfs.create_dir(dentry)
        }

        fn remove_dir(&self, dentry: &Dentry) -> Result<(), FilesystemError> {
            self.tmpfs.remove_dir(dentry)
        }

        fn remove_file(&self, dentry: &Dentry) -> Result<(), FilesystemError> {
            self.tmpfs.remove_file(dentry)
        }

        fn rename(
            &self,
            from: &Dentry,
            _to_dir: &dyn Inode,
            to: &Dentry,
            rename: Rename,
        ) -> Result<(), FilesystemError> {
            self.tmpfs.rename(from, &self.tmpfs, to, rename)
        }
    }

    /// A Tmpfs storing names in lower case and reaching each by any
    /// case, as a host that folds case does, answering each call by the
    /// folded path from its root; each directory it opens folds case too.
    struct FoldsCase {
        root: KernPath,
        at: String,
    }

    impl FoldsCase {
        fn new(tmpfs: Tmpfs) -> Self {
            Self {
                root: at(tmpfs),
                at: String::new(),
            }
        }

        /// The name `dentry` carries, folded, beneath this directory.
        fn path(&self, dentry: &Dentry) -> String {
            let name = dentry.name().to_lowercase();
            match self.at.as_str() {
                "" => name,
                at => format!("{at}/{name}"),
            }
        }
    }

    impl Inode for FoldsCase {
        holds_no_links!();

        fn open(
            &self,
            dentry: &Dentry,
            options: &OpenOptions,
        ) -> Result<Box<dyn File>, FilesystemError> {
            self.root.open(&self.path(dentry), options)
        }

        fn open_dir(&self, dentry: &Dentry) -> Result<Arc<dyn Inode>, FilesystemError> {
            let at = self.path(dentry);
            self.root.open_dir(&at)?;
            Ok(Arc::new(Self {
                root: self.root.clone(),
                at,
            }))
        }

        fn metadata(&self) -> Result<Metadata, FilesystemError> {
            self.root.symlink_metadata(&self.at)
        }

        fn symlink_metadata(&self, dentry: &Dentry) -> Result<Metadata, FilesystemError> {
            self.root.symlink_metadata(&self.path(dentry))
        }

        fn stored_name(&self, dentry: &Dentry) -> Result<String, FilesystemError> {
            Ok(dentry.name().to_lowercase())
        }

        fn entries(&self) -> Result<Vec<DirEntry>, FilesystemError> {
            match self.at.as_str() {
                "" => self.root.entries(),
                at => self.root.open_dir(at)?.entries(),
            }
        }

        fn create_dir(&self, dentry: &Dentry) -> Result<(), FilesystemError> {
            self.root.create_dir(&self.path(dentry))
        }

        fn remove_dir(&self, dentry: &Dentry) -> Result<(), FilesystemError> {
            self.root.remove_dir(&self.path(dentry))
        }

        fn remove_file(&self, dentry: &Dentry) -> Result<(), FilesystemError> {
            self.root.remove_file(&self.path(dentry))
        }

        fn rename(
            &self,
            from: &Dentry,
            to_dir: &dyn Inode,
            to: &Dentry,
            rename: Rename,
        ) -> Result<(), FilesystemError> {
            let target = (to_dir as &dyn Any)
                .downcast_ref::<Self>()
                .ok_or(FilesystemError::CrossesDevices)?;
            self.root
                .rename(&self.path(from), &self.root, &target.path(to), rename)
        }
    }

    /// What `path` names through `fs`, or why it names nothing.
    fn kind(fs: &KernPath, path: &str) -> Result<FileType, FilesystemError> {
        fs.symlink_metadata(path).map(|metadata| metadata.file_type)
    }

    // The inner directory is opened too, so only directories held as the
    // Layers' directories themselves, not reached again by their path,
    // take it along.
    // @behavior FS-110
    #[test]
    fn a_directory_opened_from_an_overlay_follows_it_when_it_moves() {
        let upper = Arc::new(Tmpfs::new());
        let overlay = at(Overlay::new().layer(Filesystem::new(upper.clone()), Access::ReadWrite));
        overlay.create_dir("dir").expect("dir must be made");
        overlay.create_dir("dir/inner").expect("inner must be made");
        let dir = overlay.open_dir("dir").expect("dir must open");
        let inner = overlay.open_dir("dir/inner").expect("inner must open");
        overlay
            .rename("dir", &overlay, "moved", Rename::Any)
            .expect("the directory must move");
        overwrite(&dir, "a", b"x");
        overwrite(&inner, "b", b"x");
        assert_eq!(
            (
                kind(&overlay, "moved/a"),
                kind(&overlay, "moved/inner/b"),
                kind(&overlay, "dir")
            ),
            (
                Ok(FileType::File),
                Ok(FileType::File),
                Err(FilesystemError::NotFound)
            ),
            "files made through directories Overlay::open_dir opened before Overlay::rename moved \
             the outer one must stand beneath its new path"
        );
    }

    // @behavior FS-111
    #[test]
    fn an_overlay_directory_removed_while_opened_makes_nothing_more() {
        let overlay =
            at(Overlay::new().layer(Filesystem::new(Arc::new(Tmpfs::new())), Access::ReadWrite));
        overlay.create_dir("dir").expect("dir must be made");
        let dir = overlay.open_dir("dir").expect("dir must open");
        overlay.remove_dir("dir").expect("dir must remove");
        overlay.create_dir("dir").expect("another dir must be made");
        assert_eq!(
            (
                write(&dir, "made", b"x"),
                dir.entries().map(|entries| entries.len()),
                names(&overlay, "dir").len()
            ),
            (Err(FilesystemError::NotFound), Ok(0), 0),
            "a directory Overlay::open_dir opened, once Overlay::remove_dir removed it and another \
             was made in its place, must make nothing, list nothing, and leave the new one empty"
        );
    }

    // A change made to a Layer outside its Overlay is the host's, so the
    // opened directory keeps the Layer's directory it holds, as a
    // descriptor keeps its directory, rather than reach the one made in
    // its place.
    // @behavior FS-112
    #[test]
    fn an_overlay_directory_keeps_its_layers_directory_through_a_change_outside_it() {
        let upper = tmpfs_with("dir/file", "x");
        let overlay = at(Overlay::new().layer(Filesystem::new(upper.clone()), Access::ReadWrite));
        let dir = overlay.open_dir("dir").expect("dir must open");
        let outside = on(&upper);
        outside
            .rename("dir", &outside, "moved", Rename::Any)
            .expect("the Layer must move it");
        outside
            .create_dir("dir")
            .expect("the Layer must make another");
        overwrite(&outside, "dir/file", b"other");
        assert_eq!(
            read(&dir, "file"),
            b"x".to_vec(),
            "a read through a directory Overlay::open_dir opened, once its Layer moved it away \
             and made another in its place outside the Overlay, must read the directory it \
             opened"
        );
    }

    // Only the lower Layer holds `dir` when both handles open it, so the
    // second sees the file only if the copy up through the first reaches
    // it too.
    // @behavior FS-119
    #[test]
    fn a_copy_up_through_one_overlay_directory_is_seen_through_another() {
        let lower: Arc<dyn Inode> = tmpfs_with("dir/a.txt", "a");
        let overlay = at(Overlay::new()
            .layer(Filesystem::new(Arc::new(Tmpfs::new())), Access::ReadWrite)
            .layer(Filesystem::new(lower), Access::Read));
        let first = overlay.open_dir("dir").expect("dir must open");
        let second = overlay.open_dir("dir").expect("dir must open again");
        write(&first, "b.txt", b"b").expect("the write must copy dir up");
        assert_eq!(
            read(&second, "b.txt"),
            b"b".to_vec(),
            "a read through one directory Overlay::open_dir opened, of a file written through \
             another opened before the write copied it up, must read the file"
        );
    }

    // The writable Layer below holds no `dir`, so reaching it again by the
    // path it was opened at would miss where it moved.
    // @behavior FS-120
    #[test]
    fn a_write_through_a_moved_overlay_directory_lands_where_it_went() {
        let upper = tmpfs_with("dir/a.txt", "a");
        let overlay = at(Overlay::new()
            .layer(Filesystem::new(upper.clone()), Access::ReadWrite)
            .layer(Filesystem::new(Arc::new(Tmpfs::new())), Access::ReadWrite));
        let dir = overlay.open_dir("dir").expect("dir must open");
        overlay
            .rename("dir", &overlay, "moved", Rename::Any)
            .expect("the directory must move");
        assert_eq!(
            (write(&dir, "b.txt", b"b"), read(&on(&upper), "moved/b.txt")),
            (Ok(()), b"b".to_vec()),
            "a write through a directory Overlay::open_dir opened, once Overlay::rename moved it \
             over a second writable Layer, must land where it moved"
        );
    }

    // @behavior FS-083
    #[test]
    fn an_overlay_spells_a_path_as_the_layer_showing_it_stores_it() {
        let overlay = at(Overlay::new()
            .layer(Filesystem::new(Arc::new(Tmpfs::new())), Access::ReadWrite)
            .layer(
                Filesystem::new(Arc::new(FoldsCase::new(
                    [("private/secret", "x")].into_iter().collect(),
                ))),
                Access::Read,
            ));
        let private = overlay
            .open_dir("PRIVATE")
            .expect("the directory must open in any case");
        assert_eq!(
            (
                overlay.stored_name("PRIVATE"),
                private.stored_name("SECRET"),
                private.stored_name("New")
            ),
            (
                Ok("private".to_string()),
                Ok("secret".to_string()),
                Ok("New".to_string())
            ),
            "a name asked of Overlay::stored_name in another case, shown by a Layer folding case, \
             must come back as that Layer stores it, a new name kept as given"
        );
    }

    // A whiteout the Layer cannot read would otherwise count as absent and
    // let the lower file show.
    // @behavior FS-079
    #[test]
    fn a_layer_that_cannot_read_its_markers_fails_the_lookup() {
        let top: Tmpfs = [(".wh.file", "")].into_iter().collect();
        let overlay = at(Overlay::new()
            .layer(Filesystem::new(Arc::new(ReadsNoMarker(top))), Access::Read)
            .layer(Filesystem::new(tmpfs_with("file", "hidden")), Access::Read));
        assert!(
            matches!(
                overlay.symlink_metadata("file"),
                Err(FilesystemError::Other(_))
            ),
            "a file looked up through Overlay::metadata, beneath a Layer that cannot read the \
             whiteout over it, must fail the lookup rather than show the file"
        );
    }

    // A marker in the lowest Layer hides nothing, as overlayfs reads no
    // opaque marker in its last layer.
    // @behavior FS-121
    #[test]
    fn the_lowest_layer_is_never_asked_for_markers() {
        let lowest: Tmpfs = [("dir/file", "lowest")].into_iter().collect();
        let overlay = at(Overlay::new()
            .layer(Filesystem::new(Arc::new(Tmpfs::new())), Access::ReadWrite)
            .layer(
                Filesystem::new(Arc::new(ReadsNoMarker(lowest))),
                Access::Read,
            ));
        assert_eq!(
            read(&overlay, "dir/file"),
            b"lowest",
            "a file read through Overlay::open from a lowest Layer that cannot read its markers \
             must read, since no Layer lies below for a marker to hide"
        );
    }

    // A directory only one Layer shows has nothing to merge or hide, so the
    // walk opens it as that Layer does, as overlayfs leaves it unmerged.
    // @behavior FS-122
    #[test]
    fn a_directory_one_layer_shows_is_walked_without_a_lookup() {
        let only: Tmpfs = [("dir/file", "only")].into_iter().collect();
        let overlay =
            at(Overlay::new().layer(Filesystem::new(Arc::new(LooksUpNoDir(only))), Access::Read));
        assert_eq!(
            read(&overlay, "dir/file"),
            b"only",
            "a file read through Overlay::open beneath a directory only one Layer shows, that \
             Layer answering no lookup of the directory, must read its content"
        );
    }

    // @behavior FS-080
    #[test]
    fn a_file_gone_during_a_rename_answers_not_found() {
        let lower = Vanishes {
            tmpfs: [("file", "lower")].into_iter().collect(),
            seen: AtomicBool::new(false),
        };
        let overlay = at(Overlay::new()
            .layer(Filesystem::new(Arc::new(Tmpfs::new())), Access::ReadWrite)
            .layer(Filesystem::new(Arc::new(lower)), Access::Read));
        assert_eq!(
            overlay.rename("file", &overlay, "moved", Rename::Any),
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
        let overlay = at(Overlay::new()
            .layer(
                Filesystem::new(tmpfs_with("dir/sub/file", "upper")),
                Access::ReadWrite,
            )
            .layer(Filesystem::new(tmpfs_with("dir", "lower")), Access::Read));
        assert_eq!(
            (
                overlay.symlink_metadata("dir/sub").map(|metadata| metadata.file_type),
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
        let (overlay, _) = overlay_on(Arc::new(HoldsDirectories(Tmpfs::new())));
        overlay
            .remove_file("dir/file")
            .expect("the file must be removed");
        overlay
            .create_dir("new")
            .expect("the directory must be made");
        let refused = (
            overlay.rename("new", &overlay, "dir", Rename::Directory),
            overlay.remove_dir("dir"),
        );
        assert_eq!(
            (refused, overlay.symlink_metadata("dir/file").err()),
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
            tmpfs: Arc::new(Tmpfs::new()),
            refuses_close,
        })
    }

    // @behavior FS-101
    #[test]
    fn a_file_opened_to_read_before_a_copy_up_reads_the_copy() {
        let (overlay, _) = overlay_on(Arc::new(Tmpfs::default()));
        let mut reader = open_to_read(&overlay, "dir/file");
        overwrite(&overlay, "dir/file", b"upper");
        assert_eq!(
            read_from(reader.as_mut()),
            b"upper".to_vec(),
            "a file Overlay::open handed out to read before another open copied it up must \
             read what the copy holds"
        );
    }

    // @behavior FS-102
    #[test]
    fn a_file_opened_to_read_before_a_rename_reads_the_moved_copy() {
        let (overlay, _) = overlay_on(Arc::new(Tmpfs::default()));
        let mut reader = open_to_read(&overlay, "dir/file");
        overlay
            .rename("dir/file", &overlay, "dir/moved", Rename::default())
            .expect("the rename must land");
        overwrite(&overlay, "dir/moved", b"moved");
        assert_eq!(
            read_from(reader.as_mut()),
            b"moved".to_vec(),
            "a file Overlay::open handed out to read before Overlay::rename moved it must read \
             what the moved copy holds"
        );
    }

    // The marker comes with the Layer, as an OCI layer carries one at its
    // root, so no change through the Overlay made it.
    // @behavior FS-104
    #[test]
    fn an_opaque_marker_at_a_layers_root_hides_the_layers_below() {
        let upper: Tmpfs = [(".wh..wh..opq", ""), ("kept", "upper")]
            .into_iter()
            .collect();
        let overlay = at(Overlay::new()
            .layer(Filesystem::new(Arc::new(upper)), Access::ReadWrite)
            .layer(Filesystem::new(tmpfs_with("hidden", "lower")), Access::Read));
        assert_eq!(
            (names(&overlay, ""), overlay.symlink_metadata("hidden")),
            (vec!["kept".to_string()], Err(FilesystemError::NotFound)),
            "an Overlay whose upper Layer holds an opaque marker at its root must list only that \
             Layer's entries through Overlay::read_dir and find no lower file through \
             Overlay::metadata"
        );
    }

    // @behavior FS-103
    #[test]
    fn a_file_open_to_read_keeps_what_it_held_once_removed() {
        let (overlay, _) = overlay_on(Arc::new(Tmpfs::default()));
        let mut reader = open_to_read(&overlay, "dir/file");
        overwrite(&overlay, "dir/file", b"upper");
        overlay
            .remove_file("dir/file")
            .expect("the removal must land");
        overwrite(&overlay, "dir/file", b"fresh");
        overlay
            .rename("dir/file", &overlay, "dir/moved", Rename::default())
            .expect("the rename must land");
        assert_eq!(
            read_from(reader.as_mut()),
            b"upper".to_vec(),
            "a file Overlay::open handed out to read must keep reading what it held once \
             Overlay::remove_file took it away, whatever later stands at its path"
        );
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
            read(&kern_path(upper.tmpfs.clone()), "dir/file"),
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
                on(&upper).symlink_metadata("dir/file"),
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
            .layer(Filesystem::new(tmpfs_with("note", "x")), Access::Read)
            .with_blocking(counts.clone());
        let held = overlay.lock.hold().expect("the lock must be free");
        let other = at(overlay.clone());
        let waiter = thread::spawn(move || other.symlink_metadata("note").map(|m| m.file_type));
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
        let lower: Arc<dyn Inode> = Arc::new([("dir/a.txt", "a")].into_iter().collect::<Tmpfs>());
        let overlay = at(Overlay::new()
            .layer(Filesystem::new(Arc::new(Tmpfs::new())), Access::ReadWrite)
            .layer(Filesystem::new(lower), Access::Read));
        let identity = |overlay: &KernPath| {
            let metadata = overlay
                .symlink_metadata("dir")
                .expect("the directory must stand");
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

    // The upper Layer's directory changes after the lower one is made, so
    // the two report different times and only the upper's matches.
    // @behavior FS-117
    #[test]
    fn an_overlay_reports_a_merged_directory_as_overlayfs_does() {
        let lower: Arc<Tmpfs> = Arc::new([("dir/a.txt", "a")].into_iter().collect());
        let upper: Arc<Tmpfs> = Arc::new(Tmpfs::new());
        on(&upper)
            .create_dir("dir")
            .expect("the directory must be made");
        std::thread::sleep(std::time::Duration::from_millis(10));
        write(&on(&upper), "dir/b.txt", b"b").expect("the upper Layer must take a write");
        let overlay = at(Overlay::new()
            .layer(Filesystem::new(upper.clone()), Access::ReadWrite)
            .layer(Filesystem::new(lower.clone()), Access::Read));
        let merged = overlay
            .symlink_metadata("dir")
            .expect("the directory must stand");
        let modified = |layer: &Arc<Tmpfs>| {
            on(layer)
                .symlink_metadata("dir")
                .expect("a Layer holds it")
                .modified
        };
        assert_eq!(
            (
                merged.nlink,
                merged.modified == modified(&upper),
                merged.modified == modified(&lower),
            ),
            (Some(1), true, false),
            "Overlay::metadata of a directory two Layers hold, written last in the upper, must \
             count one name and report the upper Layer's times"
        );
    }

    // @behavior FS-073
    #[test]
    fn an_overlay_names_a_directory_alike_whether_a_link_at_its_end_is_followed() {
        let layer = |name: &str| -> Arc<dyn Inode> {
            Arc::new(
                [(format!("dir/{name}"), "x")]
                    .into_iter()
                    .collect::<Tmpfs>(),
            )
        };
        let overlay = at(Overlay::new()
            .layer(Filesystem::new(layer("upper")), Access::ReadWrite)
            .layer(Filesystem::new(layer("lower")), Access::Read));
        let identity = |metadata: Result<Metadata, FilesystemError>| {
            let metadata = metadata.expect("the directory must stand");
            (metadata.dev, metadata.ino)
        };
        assert_eq!(
            identity(overlay.symlink_metadata("dir")),
            identity(overlay.symlink_metadata("dir")),
            "a directory two Layers hold, looked up through Overlay::symlink_metadata, must \
             report the identity Overlay::metadata does"
        );
    }
}
