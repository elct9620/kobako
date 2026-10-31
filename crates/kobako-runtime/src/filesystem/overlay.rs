//! `Overlay`: read-only lower Layers under an upper Tmpfs, stacked into
//! one Filesystem, as Linux overlayfs stacks a mount's layers.
//!
//! The upper must be a Tmpfs, as gVisor takes only an upper that holds
//! its marks. A removal hides what a lower Layer still shows with a
//! whiteout, and a directory made again where one was removed is marked
//! opaque, each as Linux marks it, in the upper, so they last as long as
//! it does. The VFS holds each directory a call changes, as Linux's
//! `i_rwsem` does above overlayfs, and a copy up holds the lock of what
//! it copies, so a change across several Layers is never seen half made.

mod file;
mod inode;
mod path;
mod stack;

use std::any::Any;
use std::sync::Arc;

use super::{
    DentryOperations, DirEntry, FileOperations, FileType, Filesystem, FilesystemError, Iattr,
    InodeOperations, Lookup, Metadata, OpenOptions, Tmpfs,
};
use crate::vfs::{Dentry, Inode};
use inode::{ovl_inode_of, ovl_inode_real};
use path::OverlayPath;
use stack::OverlayFs;

/// Read-only lower Layers under an upper Tmpfs, stacked into one
/// Filesystem. Two Overlays sharing an upper are not serialized against
/// each other, as overlayfs leaves two mounts sharing an upper directory
/// undefined.
///
/// Each directory of the view keeps each Layer's directory it stands for
/// on its dentry, so it is reached wherever those directories move, as a
/// descriptor reaches its directory, and a directory opened beneath it is
/// this Overlay asked about that dentry. overlayfs leaves a change made to
/// a Layer outside it undefined; here such a change is the host's, and a
/// directory reached before it keeps the Layers' directories it holds.
#[derive(Clone)]
pub struct Overlay {
    fs: OverlayFs,
    /// How many Overlays stack here, itself among them, as Linux counts
    /// a super block's `s_stack_depth`.
    stack_depth: usize,
}

/// The deepest Overlays may stack, as Linux's `FILESYSTEM_MAX_STACK_DEPTH`
/// bounds it, so a call through them cannot run out of stack.
const MAX_STACK_DEPTH: usize = 2;

/// The most lower Layers an Overlay takes, as overlayfs's `OVL_MAX_STACK`.
const MAX_LOWER_LAYERS: usize = 500;

impl Overlay {
    /// An Overlay of `lower`, topmost first, under `upper`. Each Layer is
    /// asked through that Filesystem's dentries, as overlayfs asks each
    /// layer by its own lookup. As overlayfs's mount does, it refuses no
    /// lower Layer, a read-only view of fewer than two, too many lower
    /// Layers, a stack too deep, and one instance as two Layers, as
    /// overlayfs answers overlapping layers; as gVisor's does, it refuses an
    /// upper that is no Tmpfs.
    pub fn new(
        lower: Vec<Arc<Filesystem>>,
        upper: Option<Arc<Filesystem>>,
    ) -> Result<Self, FilesystemError> {
        let layers: Vec<_> = lower.iter().chain(&upper).collect();
        let overlapping = layers
            .iter()
            .enumerate()
            .any(|(at, layer)| layers[..at].iter().any(|seen| Arc::ptr_eq(seen, layer)));
        if overlapping {
            return Err(FilesystemError::InvalidInput);
        }
        let too_few = if upper.is_some() { 1 } else { 2 };
        let upper_tmpfs = upper
            .as_ref()
            .is_none_or(|upper| (upper.root().as_ref() as &dyn Any).is::<Tmpfs>());
        let stack_depth = 1 + lower
            .iter()
            .filter_map(|layer| (layer.root().as_ref() as &dyn Any).downcast_ref::<Self>())
            .map(|overlay| overlay.stack_depth)
            .max()
            .unwrap_or(0);
        if !(too_few..=MAX_LOWER_LAYERS).contains(&lower.len())
            || !upper_tmpfs
            || stack_depth > MAX_STACK_DEPTH
        {
            return Err(FilesystemError::InvalidInput);
        }
        Ok(Self {
            fs: OverlayFs::new(lower, upper)?,
            stack_depth,
        })
    }
}

impl Overlay {
    /// The Layer's dentry the entry `dentry` answers through, and the inode
    /// the view recorded, as `ovl_getattr` asks the real path.
    fn real_of(&self, dentry: &Dentry) -> Option<(Arc<Dentry>, Arc<Inode>)> {
        let inode = dentry.inode()?;
        let real = ovl_inode_of(&inode)?.real()?;
        Some((real, inode))
    }

    fn lookup_at(&self, dentry: &Dentry) -> Result<Lookup, FilesystemError> {
        let level = self.fs.parent(dentry)?.level();
        let name = dentry.name();
        let stored = self.fs.case_exact_name(&level, &name)?;
        if stored != name {
            return Ok(Lookup::Spelled(stored));
        }
        if let Some(real) = self.fs.sole_real(&level, &name) {
            let real = real?;
            let Some(found) = real.inode() else {
                return Ok(Lookup::Absent);
            };
            let inode = found.with_ops(Arc::new(self.clone()));
            if inode.kind() != FileType::Directory {
                ovl_inode_real(&inode, real);
            }
            return Ok(Lookup::Found(inode));
        }
        let Some((found, real)) = self.fs.view_entry(&level, &name)? else {
            return Ok(Lookup::Absent);
        };
        let inode = Inode::new(&found, Arc::new(self.clone()));
        if let Some(real) = real {
            ovl_inode_real(&inode, real);
        }
        Ok(Lookup::Found(inode))
    }

    fn rename_to(
        &self,
        from: &Dentry,
        to_dir: &dyn InodeOperations,
        to: &Dentry,
    ) -> Result<(), FilesystemError> {
        (to_dir as &dyn Any)
            .downcast_ref::<Self>()
            .filter(|target| target.fs.is(&self.fs))
            .ok_or(FilesystemError::CrossesDevices)?;
        self.fs.rename(
            &self.fs.parent(from)?,
            from,
            &self.fs.parent(to)?,
            &to.name(),
        )
    }

    fn open_at(
        &self,
        dentry: &Dentry,
        options: &OpenOptions,
    ) -> Option<Result<Box<dyn FileOperations>, FilesystemError>> {
        Some(self.open_file(dentry, options))
    }

    /// Open the file `dentry` names. What this made is recorded after, as
    /// `->create` calls `d_instantiate`.
    fn open_file(
        &self,
        dentry: &Dentry,
        options: &OpenOptions,
    ) -> Result<Box<dyn FileOperations>, FilesystemError> {
        let file = self.fs.open(&self.fs.parent(dentry)?, dentry, options)?;
        dentry.lookup_inode(self)?;
        Ok(file)
    }
}

/// A name stays valid while every Layer's dentry for it does, as
/// `ovl_dentry_revalidate` asks each real dentry. Layers hold such a dentry
/// only for what the view keeps open.
impl DentryOperations for Overlay {
    fn revalidate(
        &self,
        _dir: &dyn InodeOperations,
        dentry: &Dentry,
    ) -> Result<bool, FilesystemError> {
        let name = dentry.name();
        for layer in self.fs.parent(dentry)?.level().iter().flatten() {
            if !layer.revalidate_real(&name)? {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

/// Every entry of the view answers through the Overlay, finding what a call
/// is about through the dentry it is handed.
impl InodeOperations for Overlay {
    fn dentry_ops(&self) -> Option<Arc<dyn DentryOperations>> {
        Some(Arc::new(self.clone()))
    }

    /// A directory reports its merged Layers; any other entry the Layer its
    /// name is found in.
    fn getattr(&self, dentry: &Dentry) -> Result<Metadata, FilesystemError> {
        let is_dir = dentry
            .inode()
            .is_some_and(|inode| inode.kind() == FileType::Directory);
        if is_dir {
            return self.fs.dir_metadata(&self.fs.dir(dentry)?.level());
        }
        let (real, inode) = self.real_of(dentry).ok_or(FilesystemError::NotFound)?;
        let found = real.getattr()?;
        Ok(match inode.identity() {
            Some(identity) => identity.onto(found),
            None => found,
        })
    }

    /// Copied up first, as `ovl_setattr` does, so a lower Layer stays as it
    /// was whatever its Filesystem would keep.
    fn setattr(&self, dentry: &Dentry, attr: &Iattr) -> Option<Result<(), FilesystemError>> {
        let (real, _) = self.real_of(dentry)?;
        if !self.fs.is_in_upper(&real) {
            if let Err(err) = self.fs.copy_up(dentry) {
                return Some(Err(err));
            }
        }
        let (real, _) = self.real_of(dentry)?;
        real.inode()?.ops().setattr(&real, attr)
    }

    /// The entry as a Layer spells it, as `ovl_lookup` hands
    /// `d_splice_alias` the inode it made.
    fn lookup(&self, dentry: &Dentry) -> Option<Result<Lookup, FilesystemError>> {
        Some(self.lookup_at(dentry))
    }

    fn get_link(&self, dentry: &Dentry) -> Option<Result<String, FilesystemError>> {
        Some(
            self.fs
                .parent(dentry)
                .and_then(|parent| self.fs.read_link(&parent.level(), &dentry.name())),
        )
    }

    fn open(
        &self,
        dentry: &Dentry,
        options: &OpenOptions,
    ) -> Option<Result<Box<dyn FileOperations>, FilesystemError>> {
        if options.directory {
            let _real = self.fs.dir(dentry).map_or_else(
                |_| Vec::new(),
                |dir| {
                    dir.level()
                        .iter()
                        .flatten()
                        .flat_map(OverlayPath::open_real)
                        .collect()
                },
            );
            let fs = self.fs.clone();
            return Some(Ok(Box::new(OverlayDir { fs, _real })));
        }
        self.open_at(dentry, options)
    }

    fn create(
        &self,
        dentry: &Dentry,
        options: &OpenOptions,
    ) -> Option<Result<Box<dyn FileOperations>, FilesystemError>> {
        self.open_at(dentry, options)
    }

    fn mkdir(&self, dentry: &Dentry) -> Option<Result<(), FilesystemError>> {
        Some(
            self.fs
                .parent(dentry)
                .and_then(|parent| self.fs.create_dir(&parent, &dentry.name())),
        )
    }

    fn rmdir(&self, dentry: &Dentry) -> Option<Result<(), FilesystemError>> {
        Some(
            self.fs
                .parent(dentry)
                .and_then(|parent| self.fs.remove_dir(&parent, &dentry.name())),
        )
    }

    fn unlink(&self, dentry: &Dentry) -> Option<Result<(), FilesystemError>> {
        Some(
            self.fs
                .parent(dentry)
                .and_then(|parent| self.fs.remove_file(&parent, &dentry.name())),
        )
    }

    /// Both directories lie in one Overlay, however many directories of
    /// it are open.
    fn rename(
        &self,
        from: &Dentry,
        to_dir: &dyn InodeOperations,
        to: &Dentry,
    ) -> Option<Result<(), FilesystemError>> {
        Some(self.rename_to(from, to_dir, to))
    }
}

/// A directory of the view, open: it merges its Layers each time it is
/// listed, finding them through the dentry it is handed, and holds each Layer's
/// directory open while it is, as `struct ovl_dir_file` holds its `realfile`.
struct OverlayDir {
    fs: OverlayFs,
    _real: Vec<Box<dyn FileOperations>>,
}

impl FileOperations for OverlayDir {
    fn iterate_shared(&mut self, dentry: &Dentry) -> Result<Vec<DirEntry>, FilesystemError> {
        self.fs.entries(&self.fs.dir(dentry)?.level())
    }

    /// Its Layers answer for what they hold, and its upper is a Tmpfs, so
    /// nothing is left to make durable, as `ovl_dir_fsync` finds with no upper
    /// of its own to sync.
    fn fsync(&mut self) -> Result<(), FilesystemError> {
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
    use super::super::wrap::{Hook, Wrap};
    use super::super::{FileType, Kiocb};
    use super::*;
    use crate::vfs::kern_path::{kern_path, KernPath};

    /// `filesystem` reached through the VFS.
    fn at(filesystem: impl InodeOperations) -> KernPath {
        kern_path(Arc::new(filesystem))
    }

    /// The Filesystem `filesystem` holds, reached through the VFS.
    fn on<F: InodeOperations>(filesystem: &Arc<F>) -> KernPath {
        kern_path(filesystem.clone())
    }

    // @behavior FS-140
    #[test]
    fn a_file_with_nothing_to_flush_syncs() {
        let lower: Tmpfs = [("file", "x")].into_iter().collect();
        let upper = Tmpfs::new();
        let overlay = at(Overlay::new(
            vec![Filesystem::new(Arc::new(lower))],
            Some(Filesystem::new(Arc::new(upper))),
        )
        .expect("the Overlay must stack"));
        assert_eq!(
            open_to_read(&overlay, "file").fsync(),
            Ok(()),
            "FileOperations::fsync on an Overlay's file open to read from a Tmpfs Layer must \
             succeed"
        );
        let tmpfs = at([("file", "x")].into_iter().collect::<Tmpfs>());
        assert_eq!(
            open_to_read(&tmpfs, "file").fsync(),
            Ok(()),
            "FileOperations::fsync on a Tmpfs file must succeed"
        );
    }

    fn open_to_read(fs: &KernPath, path: &str) -> Box<dyn FileOperations> {
        fs.open(
            path,
            &OpenOptions {
                read: true,
                ..OpenOptions::default()
            },
        )
        .expect("the file must open to read")
    }

    fn read_from(file: &mut dyn FileOperations) -> Vec<u8> {
        let mut buf = vec![0; 64];
        let read = file
            .read_iter(&mut Kiocb::at(0), &mut buf)
            .expect("the file must read");
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
        .write_iter(&mut Kiocb::at(0), contents)
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

    /// `upper` over the one lower Layer `lower`.
    fn over(lower: Arc<dyn InodeOperations>, upper: Arc<Tmpfs>) -> Overlay {
        Overlay::new(vec![Filesystem::new(lower)], Some(Filesystem::new(upper)))
            .expect("an upper over a lower Layer must make a view")
    }

    /// The lower Layers `top` over `bottom`, with no upper.
    fn lowers(top: Arc<dyn InodeOperations>, bottom: Arc<dyn InodeOperations>) -> Overlay {
        Overlay::new(vec![Filesystem::new(top), Filesystem::new(bottom)], None)
            .expect("two lower Layers must make a view")
    }

    /// `upper` over a lower Tmpfs holding `dir/file` = "lower".
    fn overlay_on(upper: Arc<Tmpfs>) -> (KernPath, Arc<Tmpfs>) {
        let lower = tmpfs_with("dir/file", "lower");
        (at(over(lower.clone(), upper)), lower)
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

        /// An empty upper over the host directory as the lower Layer.
        fn overlay(&self) -> KernPath {
            self.under(Arc::new(Tmpfs::new()))
        }

        /// The host directory as the lower Layer beneath `upper`.
        fn under(&self, upper: Arc<Tmpfs>) -> KernPath {
            at(over(Arc::new(self.bind()), upper))
        }

        fn bind(&self) -> super::super::Bind {
            super::super::Bind::new(&self.0).expect("the host directory must open")
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
        let overlay = tree.under(tmpfs_with("upper/dirlink/.keep", ""));
        overlay
            .remove_file("dirlink/.keep")
            .expect("the seed must go");
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
                overlay.rename("pipe", &overlay, "moved")
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
            .expect("a directory must be made in the upper");
        let file_type = |path| kind(&overlay, path);
        assert_eq!(
            (
                overlay.rename("pipe", &overlay, "dir"),
                overlay.rename("made", &overlay, "pipe"),
                file_type("pipe"),
                file_type("made"),
            ),
            (
                Err(FilesystemError::IsADirectory),
                Err(FilesystemError::NotADirectory),
                Ok(FileType::Fifo),
                Ok(FileType::Directory)
            ),
            "a FIFO renamed onto a directory through an Overlay must be refused as a directory, \
             and a directory renamed onto a FIFO as no directory, leaving both in place"
        );
    }

    /// A lower Layer whose one file, `file`, would keep any time it is asked
    /// to, recording that it was.
    struct KeepsTimes(Arc<AtomicUsize>);

    impl InodeOperations for KeepsTimes {
        fn getattr(&self, dentry: &Dentry) -> Result<Metadata, FilesystemError> {
            Ok(match dentry.name().as_str() {
                "file" => Metadata::new(FileType::File, 0).with_identity(1, 2),
                _ => Metadata::new(FileType::Directory, 0).with_identity(1, 1),
            })
        }

        fn lookup(&self, dentry: &Dentry) -> Option<Result<Lookup, FilesystemError>> {
            Some(Ok(match dentry.name().as_str() {
                "file" => Lookup::Found(Inode::new(
                    &Metadata::new(FileType::File, 0).with_identity(1, 2),
                    Arc::new(Self(self.0.clone())),
                )),
                _ => Lookup::Absent,
            }))
        }

        fn setattr(&self, _dentry: &Dentry, _attr: &Iattr) -> Option<Result<(), FilesystemError>> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Some(Ok(()))
        }

        fn open(
            &self,
            _dentry: &Dentry,
            _options: &OpenOptions,
        ) -> Option<Result<Box<dyn FileOperations>, FilesystemError>> {
            struct Empty;
            impl FileOperations for Empty {
                fn read_iter(
                    &mut self,
                    _iocb: &mut Kiocb,
                    _buf: &mut [u8],
                ) -> Result<usize, FilesystemError> {
                    Ok(0)
                }
            }
            Some(Ok(Box::new(Empty)))
        }
    }

    // Every built-in Filesystem refuses a time, so only one keeping it shows
    // the call stopped at the upper.
    // @behavior FS-147
    #[test]
    fn a_time_set_through_an_overlay_leaves_its_lower_layer_as_it_was() {
        let asked = Arc::new(AtomicUsize::new(0));
        let upper = Arc::new(Tmpfs::new());
        let overlay = Overlay::new(
            vec![Filesystem::new(Arc::new(KeepsTimes(asked.clone())))],
            Some(Filesystem::new(upper.clone())),
        )
        .unwrap_or_else(|_| panic!("the Overlay must make"));
        let mount = crate::mount::Mount::new(
            "/data",
            Filesystem::new(Arc::new(overlay)),
            crate::mount::Access::ReadWrite,
        )
        .expect("/data is a guest path");
        let answer = crate::vfs::do_utimes(
            &Arc::default(),
            &crate::vfs::Path::root(mount.mnt()),
            "file",
            false,
            &Iattr::times(None, Some(crate::filesystem::SetTime::Now)),
        );
        assert_eq!(
            (
                answer,
                asked.load(Ordering::SeqCst),
                kern_path(upper).symlink_metadata("upper/file").is_ok()
            ),
            (
                Err(crate::vfs::Error::Filesystem(FilesystemError::Unsupported)),
                0,
                true
            ),
            "vfs::do_utimes through an Overlay on a file of its lower Layer must copy the file \
             up, answer what the upper answers, and never ask the lower Layer"
        );
    }

    // A recursive climb needs more than this stack at this depth, in a build
    // of any optimization, so only a loop passes.
    // @behavior FS-146
    #[test]
    fn a_change_copies_up_however_deep_it_lands() {
        let deep = vec!["a"; 1500].join("/");
        let lower: Tmpfs = [(format!("{deep}/file"), "x")].into_iter().collect();
        let overlay = Overlay::new(
            vec![Filesystem::new(Arc::new(lower))],
            Some(Filesystem::new(Arc::new(Tmpfs::new()))),
        )
        .unwrap_or_else(|_| panic!("the Overlay must make"));
        let made = std::thread::Builder::new()
            .stack_size(256 * 1024)
            .spawn(move || {
                let view = kern_path(Arc::new(overlay));
                view.create_dir(&format!("{deep}/made"))?;
                view.symlink_metadata(&format!("{deep}/made"))
                    .map(|found| found.file_type)
            })
            .expect("the thread must start")
            .join()
            .expect("the thread must not panic");
        assert_eq!(
            made,
            Ok(FileType::Directory),
            "a directory made 1500 directories deep in a lower Layer, through an Overlay on a \
             256 KiB stack, must copy every directory above it up and stand"
        );
    }

    // The upper is a Tmpfs, which holds no link, so a link cannot land there.
    // @behavior FS-051
    #[cfg(unix)]
    #[test]
    fn an_overlay_refuses_to_move_a_link_its_read_only_layer_holds() {
        let host = LinkTree::new("through");
        let overlay = host.overlay();
        assert_eq!(
            (
                overlay.rename("link", &overlay, "moved"),
                read(&overlay, "target")
            ),
            (Err(FilesystemError::CrossesDevices), b"lower".to_vec()),
            "a rename of a link a read-only Bind Layer holds, through an Overlay, must be refused \
             as crossing devices and leave the target as it was"
        );
    }

    // The lower Tmpfs holds a readable `dangling`, so only the upper
    // link standing at that path can keep the read from reaching it.
    // @behavior FS-053
    #[cfg(unix)]
    #[test]
    fn a_link_a_layer_holds_hides_what_the_layers_below_hold_there() {
        let host = LinkTree::new("shadow");
        let overlay = at(lowers(
            Arc::new(host.bind()),
            tmpfs_with("dangling", "lower"),
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
            Err(FilesystemError::FilesystemLoop),
            "a lower file beneath a dangling link a higher Bind Layer holds at its path, \
             opened through Overlay::open, must be refused as the link, the file unreached"
        );
    }

    // The lower Tmpfs holds `dirlink/x` beside where the upper link points,
    // so only the link hiding everything beneath it keeps `x` out.
    // @behavior FS-057
    #[cfg(unix)]
    #[test]
    fn a_link_a_layer_holds_hides_what_the_layers_below_hold_beneath_it() {
        let host = LinkTree::new("shadow-beneath");
        let overlay = at(lowers(
            Arc::new(host.bind()),
            tmpfs_with("dirlink/x", "lower"),
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

    // The lower directory stays in its Layer, so only the whiteout keeps
    // it from showing once the upper file goes.
    // @behavior FS-058
    #[test]
    fn removing_a_file_over_a_lower_directory_leaves_it_hidden() {
        let lower = tmpfs_with("f/g", "lower");
        let overlay = at(over(lower.clone(), tmpfs_with("upper/f", "upper")));
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

    // The upper's `dir` holds only the whiteout hiding `file`, so
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
        let moved = overlay.rename("new", &overlay, "dir");
        assert_eq!(
            (moved, names(&overlay, "dir")),
            (Ok(()), vec!["made".to_string()]),
            "a directory renamed through Overlay::rename onto one the view shows empty, whose \
             upper copy holds only a whiteout, must take its place and show only its own entries"
        );
    }

    // The directory made again is marked opaque in the upper, so only that
    // mark tells it apart from one the lower Layer still holds.
    // @behavior FS-060
    #[test]
    fn a_directory_made_again_moves_as_the_uppers_own() {
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
        let moved = overlay.rename("dir", &overlay, "moved");
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
            "a directory made again through an Overlay where a lower Layer's one was removed, \
             renamed through Overlay::rename, must move and leave nothing of the lower one behind"
        );
    }

    // @behavior FS-061
    #[test]
    fn an_overlay_refuses_a_file_moved_beneath_itself_as_no_directory() {
        let (overlay, _) = overlay_on(Arc::new(Tmpfs::new()));
        assert_eq!(
            overlay.rename("dir/file", &overlay, "dir/file/x"),
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
            "an exclusive create and a create_dir at a dangling link a lower Bind Layer \
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
    fn a_write_to_a_lower_layers_file_lands_in_the_upper() {
        let upper = Arc::new(Tmpfs::new());
        let (overlay, _) = overlay_on(upper.clone());
        overwrite(&overlay, "dir/file", b"upper");
        assert_eq!(
            read(&on(&upper), "upper/dir/file"),
            b"upper",
            "a lower Layer's file written through Overlay::open must land in the upper"
        );
    }

    // @behavior FS-010
    #[test]
    fn an_overlay_leaves_a_lower_layer_as_it_was() {
        let (overlay, lower) = overlay_on(Arc::new(Tmpfs::new()));
        overwrite(&overlay, "dir/file", b"upper");
        overlay
            .remove_file("dir/file")
            .expect("the file must be removed");
        assert_eq!(
            read(&on(&lower), "dir/file"),
            b"lower",
            "a lower Layer's file written and removed through an Overlay must stay as it was"
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
        let overlay = at(lowers(
            tmpfs_with("note", "upper"),
            tmpfs_with("note", "lower"),
        ));
        assert_eq!(
            read(&overlay, "note"),
            b"upper",
            "a path both Layers hold read through Overlay::open must read the upper Layer's content"
        );
    }

    // @behavior FS-014
    #[test]
    fn a_write_to_the_uppers_file_changes_it_in_place() {
        let upper = tmpfs_with("upper/note", "upper");
        let overlay = at(over(tmpfs_with("note", "lower"), upper.clone()));
        overwrite(&overlay, "note", b"changed");
        assert_eq!(
            read(&on(&upper), "upper/note"),
            b"changed",
            "the upper's file written through Overlay::open must change in place"
        );
    }

    // @behavior FS-015
    #[test]
    fn a_new_file_lands_in_the_upper() {
        let upper = Arc::new(Tmpfs::new());
        let lower = Arc::new(Tmpfs::new());
        let overlay = at(over(lower.clone(), upper.clone()));
        overwrite(&overlay, "new", b"made");
        assert_eq!(
            (
                read(&on(&upper), "upper/new"),
                on(&lower).symlink_metadata("new")
            ),
            (b"made".to_vec(), Err(FilesystemError::NotFound)),
            "a file created through Overlay::open must land in the upper alone"
        );
    }

    // @behavior FS-016
    #[test]
    fn a_removal_leaves_a_whiteout_in_the_upper() {
        let upper = Arc::new(Tmpfs::new());
        let (overlay, _) = overlay_on(upper.clone());
        overlay
            .remove_file("dir/file")
            .expect("the file must be removed");
        assert_eq!(
            on(&upper)
                .symlink_metadata("upper/dir/file")
                .map(|metadata| metadata.file_type),
            Ok(FileType::CharDevice),
            "a lower Layer's file removed through Overlay::remove_file must leave a whiteout, a \
             device of no kind the guest opens, at its name in the upper"
        );
    }

    // @behavior FS-017
    #[test]
    fn a_whiteout_lasts_as_long_as_the_upper() {
        let upper = Arc::new(Tmpfs::new());
        let (overlay, lower) = overlay_on(upper.clone());
        overlay
            .remove_file("dir/file")
            .expect("the file must be removed");
        let rebuilt = at(over(lower, upper));
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
            "a directory made again through Overlay::create_dir must list nothing from the lower Layer"
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
            "a directory holding a whiteout listed through Overlay::read_dir must not list the \
             whited-out name"
        );
    }

    // A whiteout is a device rather than a name, so no name is the
    // Overlay's to reserve.
    // @behavior FS-020
    #[test]
    fn a_name_starting_with_wh_is_an_ordinary_name() {
        let (overlay, _) = overlay_on(Arc::new(Tmpfs::new()));
        overwrite(&overlay, "dir/.wh.file", b"made");
        assert_eq!(
            (read(&overlay, "dir/.wh.file"), read(&overlay, "dir/file")),
            (b"made".to_vec(), b"lower".to_vec()),
            "a file named .wh.file created through Overlay::open must be made, and must hide \
             nothing"
        );
    }

    // @behavior FS-021
    #[test]
    fn an_overlay_with_no_upper_refuses_every_change_as_read_only() {
        let overlay = at(lowers(
            tmpfs_with("dir/file", "lower"),
            Arc::new(Tmpfs::new()),
        ));
        assert_eq!(
            write(&overlay, "dir/file", b"x"),
            Err(FilesystemError::ReadOnlyFilesystem),
            "a file written through an Overlay with no upper must be refused as read-only"
        );
    }

    /// Why `Overlay::new` refuses `lower` under `upper`, or `None`.
    fn refusal(
        lower: Vec<Arc<dyn InodeOperations>>,
        upper: Option<Arc<dyn InodeOperations>>,
    ) -> Option<FilesystemError> {
        Overlay::new(
            lower.into_iter().map(Filesystem::new).collect(),
            upper.map(Filesystem::new),
        )
        .err()
    }

    // gVisor takes only an upper that holds its markers, and only a
    // Tmpfs does here.
    // @behavior FS-123
    #[test]
    fn an_overlay_refuses_an_upper_that_is_no_tmpfs() {
        let host = std::env::temp_dir();
        let bind = super::super::Bind::new(&host).expect("the host directory must open");
        assert_eq!(
            refusal(vec![Arc::new(Tmpfs::new())], Some(Arc::new(bind))),
            Some(FilesystemError::InvalidInput),
            "Overlay::new with a Bind as its upper must be refused as invalid input"
        );
    }

    // Nothing else tells these apart: every other rule holds of each list, and
    // a view over them would show the upper's own workdir.
    // @behavior FS-148
    #[test]
    fn an_overlay_refuses_one_instance_as_two_layers() {
        let instance = || Filesystem::new(Arc::new(Tmpfs::new()));
        let (shared, doubled) = (instance(), instance());
        assert_eq!(
            (
                Overlay::new(vec![shared.clone()], Some(shared)).err(),
                Overlay::new(vec![doubled.clone(), doubled, instance()], None).err(),
            ),
            (
                Some(FilesystemError::InvalidInput),
                Some(FilesystemError::InvalidInput),
            ),
            "Overlay::new with one instance as its upper and a lower Layer, or as two lower \
             Layers, must be refused as invalid input"
        );
    }

    // @behavior FS-124
    #[test]
    fn an_overlay_without_an_upper_needs_two_lower_layers() {
        assert_eq!(
            refusal(vec![Arc::new(Tmpfs::new())], None),
            Some(FilesystemError::InvalidInput),
            "Overlay::new with one lower Layer and no upper must be refused as invalid input"
        );
    }

    // Two Overlays deep is what Linux allows, so only a third is refused.
    // @behavior FS-126
    #[test]
    fn an_overlay_stacks_at_most_two_deep() {
        let first: Arc<dyn InodeOperations> =
            Arc::new(lowers(Arc::new(Tmpfs::new()), Arc::new(Tmpfs::new())));
        let second: Arc<dyn InodeOperations> =
            Arc::new(over(first.clone(), Arc::new(Tmpfs::new())));
        assert_eq!(
            (
                refusal(vec![first], Some(Arc::new(Tmpfs::new()))),
                refusal(vec![second], Some(Arc::new(Tmpfs::new())))
            ),
            (None, Some(FilesystemError::InvalidInput)),
            "Overlay::new over an Overlay must make a view, and over an Overlay over an Overlay \
             must be refused as invalid input"
        );
    }

    // @behavior FS-127
    #[test]
    fn an_overlay_takes_at_most_five_hundred_lower_layers() {
        let layers = |count: usize| -> Vec<Arc<dyn InodeOperations>> {
            (0..count)
                .map(|_| Arc::new(Tmpfs::new()) as Arc<dyn InodeOperations>)
                .collect()
        };
        assert_eq!(
            (refusal(layers(500), None), refusal(layers(501), None)),
            (None, Some(FilesystemError::InvalidInput)),
            "Overlay::new must take 500 lower Layers and refuse 501 as invalid input"
        );
    }

    // @behavior FS-125
    #[test]
    fn an_overlay_needs_a_lower_layer() {
        assert_eq!(
            refusal(Vec::new(), Some(Arc::new(Tmpfs::new()))),
            Some(FilesystemError::InvalidInput),
            "Overlay::new with an upper and no lower Layer must be refused as invalid input"
        );
    }

    #[test]
    fn an_overlay_lists_every_layer_without_what_it_hid() {
        let (overlay, _) = overlay_on(Arc::new(Tmpfs::new()));
        overwrite(&overlay, "dir/new", b"made");
        assert_eq!(names(&overlay, "dir"), ["file", "new"]);
    }

    #[test]
    fn a_lower_file_renamed_through_an_overlay_moves_into_the_upper() {
        let upper = Arc::new(Tmpfs::new());
        let (overlay, lower) = overlay_on(upper.clone());
        overlay
            .rename("dir/file", &overlay, "dir/moved")
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

    /// Another call through the VFS at `overlay`, started once on a thread
    /// of its own and given a while to finish, recording whether it did
    /// before the call starting it went on.
    #[derive(Default)]
    struct Interleave {
        overlay: Mutex<Option<crate::vfs::Path>>,
        finished_inside: Mutex<Option<bool>>,
        other: Mutex<Option<thread::JoinHandle<()>>>,
    }

    impl Interleave {
        fn start(&self) {
            let Some(overlay) = self.overlay.lock().unwrap().take() else {
                return;
            };
            let finished = Arc::new(AtomicUsize::new(0));
            let marker = finished.clone();
            let handle = thread::spawn(move || {
                let how = crate::vfs::OpenHow {
                    write: true,
                    create: true,
                    ..crate::vfs::OpenHow::default()
                };
                let _ = crate::vfs::do_file_open(&Arc::default(), &overlay, "dir/new", &how);
                marker.store(1, Ordering::SeqCst);
            });
            let deadline = Instant::now() + Duration::from_millis(200);
            while finished.load(Ordering::SeqCst) == 0 && Instant::now() < deadline {
                thread::sleep(Duration::from_millis(5));
            }
            *self.finished_inside.lock().unwrap() = Some(finished.load(Ordering::SeqCst) == 1);
            *self.other.lock().unwrap() = Some(handle);
        }
    }

    /// A Tmpfs whose directory beneath its root starts `Interleave` the first
    /// time one is listed, as a removal's emptiness check lists it.
    impl Hook for Interleave {
        fn open(
            &self,
            inner: &dyn InodeOperations,
            dentry: &Dentry,
            options: &OpenOptions,
        ) -> Option<Result<Box<dyn FileOperations>, FilesystemError>> {
            if options.directory && dentry.parent().is_some() {
                self.start();
            }
            inner.open(dentry, options)
        }
    }

    // The other call starts inside this one, between its emptiness check
    // and its removal, so only the Overlay's lock keeps the file it
    // creates from being removed with the directory.
    // @behavior FS-037
    #[test]
    fn a_change_through_an_overlay_waits_for_the_one_in_progress() {
        let tmpfs = tmpfs_with("dir/.keep", "");
        let interleave = Arc::new(Interleave::default());
        on(&tmpfs)
            .remove_file("dir/.keep")
            .expect("the seed must go");
        let mount = crate::mount::Mount::new(
            "/data",
            Filesystem::new(Arc::new(over(
                Arc::new(Wrap::new(tmpfs, interleave.clone())),
                Arc::new(Tmpfs::new()),
            ))),
            crate::mount::Access::ReadWrite,
        )
        .expect("/data is a guest path");
        let root = crate::vfs::Path::root(mount.mnt());
        *interleave.overlay.lock().unwrap() = Some(root.clone());
        let removed = crate::vfs::filename_rmdir(&Arc::default(), &root, "dir").is_ok();
        interleave
            .other
            .lock()
            .unwrap()
            .take()
            .expect("the other call must have started")
            .join()
            .expect("the other call must not panic");
        assert_eq!(
            (
                removed,
                *interleave.finished_inside.lock().unwrap(),
                crate::vfs::vfs_fstatat(&Arc::default(), &root, "dir/new", false).is_ok()
            ),
            (true, Some(false), false),
            "a create through vfs::do_file_open into an Overlay directory vfs::filename_rmdir is removing must \
             wait for it, failing rather than landing in the directory being removed"
        );
    }

    // @behavior FS-038
    #[test]
    fn a_change_through_an_overlay_from_inside_one_answers_a_deadlock() {
        type Answers = (bool, Result<(), crate::vfs::Error>);

        struct Reenters {
            overlay: Mutex<Option<crate::vfs::Path>>,
            answer: Mutex<Option<Answers>>,
        }

        impl Hook for Reenters {
            fn getattr(
                &self,
                inner: &dyn InodeOperations,
                dentry: &Dentry,
            ) -> Result<Metadata, FilesystemError> {
                let reentering = self.overlay.lock().unwrap().take();
                if let Some(overlay) = reentering {
                    *self.answer.lock().unwrap() = Some((
                        crate::vfs::vfs_fstatat(&Arc::default(), &overlay, &dentry.name(), false)
                            .is_ok(),
                        crate::vfs::filename_mkdirat(&Arc::default(), &overlay, "made"),
                    ));
                }
                inner.getattr(dentry)
            }
        }

        let layer = Arc::new(Reenters {
            overlay: Mutex::new(None),
            answer: Mutex::new(None),
        });
        let tmpfs: Tmpfs = [("note", "x")].into_iter().collect();
        let mount = crate::mount::Mount::new(
            "/data",
            Filesystem::new(Arc::new(over(
                Arc::new(Wrap::new(Arc::new(tmpfs), layer.clone())),
                Arc::new(Tmpfs::new()),
            ))),
            crate::mount::Access::ReadWrite,
        )
        .expect("/data is a guest path");
        let root = crate::vfs::Path::root(mount.mnt());
        *layer.overlay.lock().unwrap() = Some(root.clone());
        crate::vfs::vfs_fstatat(&Arc::default(), &root, "note", false)
            .expect("the outer call must answer");
        let answers = layer.answer.lock().unwrap().take();
        assert_eq!(
            answers,
            Some((
                true,
                Err(crate::vfs::Error::Filesystem(FilesystemError::Deadlock))
            )),
            "a read through an Overlay made from inside one of its own reads on the same thread \
             must answer, and a change must answer Deadlock rather than wait forever"
        );
    }

    /// An upper whose `upper` is seeded with `files`, with room for
    /// `inodes` more than its root, `upper`, and `work` take, over a lower
    /// Tmpfs holding `dir/file` = "lower".
    fn full_over_lower(files: &[(&str, &str)], inodes: usize) -> (KernPath, Arc<Tmpfs>) {
        let seeded = files
            .iter()
            .map(|(path, contents)| (format!("upper/{path}"), *contents));
        let upper = Tmpfs::from_iter(seeded).with_nr_inodes(Some(inodes + 3));
        let upper = Arc::new(upper);
        (
            at(over(tmpfs_with("dir/file", "lower"), upper.clone())),
            upper,
        )
    }

    // The upper copy is all the room there is, so the whiteout cannot be
    // made; taking the copy away first would show the lower file again.
    // @behavior FS-040
    #[test]
    fn a_removal_that_cannot_leave_its_whiteout_removes_nothing() {
        let (overlay, _) = full_over_lower(&[("dir/file", "upper")], 2);
        assert_eq!(
            (overlay.remove_file("dir/file"), read(&overlay, "dir/file")),
            (Err(FilesystemError::StorageFull), b"upper".to_vec()),
            "a file removed through Overlay::remove_file whose whiteout finds no room must stay as \
             it was"
        );
    }

    // The directory is built in the workdir before it replaces the
    // whiteout, so a workdir with no room leaves the whiteout standing.
    // @behavior FS-041
    #[test]
    fn a_directory_made_over_a_whiteout_without_room_is_not_made() {
        let lower = Arc::new(Tmpfs::new());
        on(&lower)
            .create_dir("gone")
            .expect("the directory must be made");
        let upper = Arc::new(Tmpfs::new().with_nr_inodes(Some(4)));
        let overlay = at(over(lower, upper));
        overlay
            .remove_dir("gone")
            .expect("the directory must be removed");
        assert_eq!(
            (overlay.create_dir("gone"), overlay.symlink_metadata("gone")),
            (
                Err(FilesystemError::StorageFull),
                Err(FilesystemError::NotFound)
            ),
            "a directory made through Overlay::create_dir over a whiteout, with no room left, \
             must be refused as out of space and leave the name hidden"
        );
    }

    // The copy up takes the last room, so the whiteout over the source
    // cannot be made; moving first would leave the file in two places.
    // @behavior FS-042
    #[test]
    fn a_rename_that_cannot_leave_its_whiteout_moves_nothing() {
        let (overlay, _) = full_over_lower(&[], 2);
        assert_eq!(
            (
                overlay.rename("dir/file", &overlay, "dir/moved"),
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

    // @behavior FS-043
    #[test]
    fn a_directory_a_lower_layer_holds_does_not_move_through_an_overlay() {
        let (overlay, _) = overlay_on(Arc::new(Tmpfs::new()));
        assert_eq!(
            (
                overlay.rename("dir", &overlay, "moved"),
                overlay.symlink_metadata("moved")
            ),
            (
                Err(FilesystemError::CrossesDevices),
                Err(FilesystemError::NotFound)
            ),
            "a directory a lower Layer holds renamed through Overlay::rename must be refused \
             as crossing devices, as overlayfs refuses it"
        );
    }

    // The directory shows only from the lower Layer, so only a check of
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
                overlay.rename("note", &overlay, "dir"),
                overlay.rename("empty", &overlay, "dir"),
                read(&overlay, "dir/file")
            ),
            (
                Err(FilesystemError::IsADirectory),
                Err(FilesystemError::DirectoryNotEmpty),
                b"lower".to_vec()
            ),
            "a rename through Overlay::rename onto a directory only a lower Layer shows must \
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
        let overlay = at(over(lower.clone(), upper.clone()));
        overlay
            .create_dir("made")
            .expect("the directory must be made");
        overlay
            .rename("made", &overlay, "dir")
            .expect("the directory must move over the empty one");
        overwrite(&on(&lower), "dir/later", b"lower");
        assert_eq!(
            names(&overlay, "dir"),
            Vec::<String>::new(),
            "a directory moved through Overlay::rename over one a lower Layer holds must be \
             marked opaque, so a file the lower Layer gains there later does not show"
        );
    }

    // A change is built in `work` before it lands, so `work` holds nothing
    // once each change is done, and only `upper` holds what the view shows.
    // @behavior FS-128
    #[test]
    fn an_overlay_keeps_its_changes_in_its_tmpfss_upper() {
        let upper = Arc::new(Tmpfs::new());
        let (overlay, _) = overlay_on(upper.clone());
        overwrite(&overlay, "dir/file", b"changed");
        overlay
            .remove_file("dir/file")
            .expect("the file must be removed");
        assert_eq!(
            (names(&on(&upper), ""), names(&on(&upper), "work")),
            (
                vec!["upper".to_string(), "work".to_string()],
                Vec::<String>::new()
            ),
            "an Overlay's upper Tmpfs, after a copy up and a removal through it, must hold only \
             upper and an empty work"
        );
    }

    // The upper's `dir` holds the whiteout the file's removal left, so only
    // clearing it before the removal lets what is exchanged out go.
    // @behavior FS-134
    #[test]
    fn a_directory_removed_through_an_overlay_leaves_nothing_in_its_workdir() {
        let upper = Arc::new(Tmpfs::new());
        let (overlay, _) = overlay_on(upper.clone());
        overlay
            .remove_file("dir/file")
            .expect("the file must be removed");
        overlay
            .remove_dir("dir")
            .expect("the emptied directory must be removed");
        assert_eq!(
            (names(&on(&upper), "work"), kind(&overlay, "dir")),
            (Vec::<String>::new(), Err(FilesystemError::NotFound)),
            "Overlay::remove_dir of a lower directory whose file Overlay::remove_file removed \
             must leave the upper Tmpfs's work empty and the directory gone"
        );
    }

    // The open writes nothing, so only the copy taking the lower file's
    // times, rather than the time it was made, keeps them.
    // @behavior FS-129
    #[test]
    fn a_copy_up_keeps_the_lower_files_times() {
        let lower = tmpfs_with("dir/file", "lower");
        let before = on(&lower)
            .symlink_metadata("dir/file")
            .expect("the lower file must stand");
        std::thread::sleep(Duration::from_millis(10));
        let overlay = at(over(lower, Arc::new(Tmpfs::new())));
        overlay
            .open(
                "dir/file",
                &OpenOptions {
                    write: true,
                    ..OpenOptions::default()
                },
            )
            .expect("the open must copy the file up");
        assert_eq!(
            overlay
                .symlink_metadata("dir/file")
                .map(|metadata| metadata.modified),
            Ok(before.modified),
            "a lower file opened to write through Overlay::open, writing nothing, must keep the \
             lower file's modified time once copied up"
        );
    }

    // The second copy up lands in a directory the first made, so only
    // restoring its times leaves what the view reports unchanged.
    // @behavior FS-130
    #[test]
    fn a_copy_up_leaves_its_directorys_times_as_they_were() {
        let lower: Arc<Tmpfs> = Arc::new([("dir/a", "a"), ("dir/b", "b")].into_iter().collect());
        let overlay = at(over(lower, Arc::new(Tmpfs::new())));
        overwrite(&overlay, "dir/a", b"changed");
        let before = overlay
            .symlink_metadata("dir")
            .expect("the directory must stand")
            .modified;
        std::thread::sleep(Duration::from_millis(10));
        overlay
            .open(
                "dir/b",
                &OpenOptions {
                    write: true,
                    ..OpenOptions::default()
                },
            )
            .expect("the open must copy the file up");
        assert_eq!(
            overlay
                .symlink_metadata("dir")
                .map(|metadata| metadata.modified),
            Ok(before),
            "a lower file copied up through Overlay::open into a directory the upper holds must \
             leave that directory's modified time as it was"
        );
    }

    /// An Overlay over an empty lower Layer under the Tmpfs `upper`, and
    /// that same Tmpfs mounted on its own, each reached through the VFS.
    fn beside_its_upper(upper: Tmpfs) -> (crate::vfs::Path, crate::vfs::Path) {
        let upper = Filesystem::new(Arc::new(upper));
        let overlay = Overlay::new(
            vec![Filesystem::new(Arc::new(Tmpfs::new()))],
            Some(upper.clone()),
        )
        .expect("an upper over a lower Layer must make a view");
        let root = |filesystem| {
            let mount =
                crate::mount::Mount::new("/data", filesystem, crate::mount::Access::ReadWrite)
                    .expect("/data is a guest path");
            crate::vfs::Path::root(mount.mnt())
        };
        (root(Filesystem::new(Arc::new(overlay))), root(upper))
    }

    // The move holds the directory it leaves and the one above it, and the
    // removal holds the same two, so only taking the one above first in
    // both keeps them from waiting on each other; unordered, they wait
    // within a few moves.
    // @behavior FS-131
    #[test]
    fn an_overlay_and_a_mount_of_its_upper_never_wait_on_each_other() {
        let (overlay, upper) = beside_its_upper(
            [("upper/a/b/c", "c"), ("upper/a/b/keep", "")]
                .into_iter()
                .collect(),
        );
        let moved = Arc::new(AtomicBool::new(false));
        let removing = {
            let moved = moved.clone();
            thread::spawn(move || {
                while !moved.load(Ordering::SeqCst) {
                    let _ = crate::vfs::filename_rmdir(&Arc::default(), &upper, "upper/a/b");
                }
            })
        };
        let (done, finished) = std::sync::mpsc::channel();
        thread::spawn(move || {
            for _ in 0..500 {
                crate::vfs::filename_renameat2(&Arc::default(), &overlay, "a/b/c", &overlay, "a/c")
                    .expect("the file must move up");
                crate::vfs::filename_renameat2(&Arc::default(), &overlay, "a/c", &overlay, "a/b/c")
                    .expect("the file must move back");
            }
            let _ = done.send(());
        });
        let returned = finished.recv_timeout(Duration::from_secs(10)).is_ok();
        moved.store(true, Ordering::SeqCst);
        if returned {
            removing.join().expect("the removal must not panic");
        }
        assert!(
            returned,
            "vfs::filename_renameat2 of a file up one directory through an Overlay, while vfs::filename_rmdir \
             removes that directory through a Mount of its upper Tmpfs, must return"
        );
    }

    // The make holds only the directory it makes in, so only the removal
    // holding that directory too, and removing it only once empty, keeps
    // what the make made from going with it. The two land in the same
    // instant only a few times in this many tries.
    // @behavior FS-132
    #[test]
    fn a_removal_through_an_overlay_waits_for_a_change_in_that_directory() {
        let (overlay, upper) = beside_its_upper([("upper/a/.keep", "")].into_iter().collect());
        let stop = Arc::new(AtomicBool::new(false));
        let churning = {
            let stop = stop.clone();
            thread::spawn(move || {
                while !stop.load(Ordering::SeqCst) {
                    let _ = crate::vfs::filename_mkdirat(&Arc::default(), &overlay, "a/b");
                    let _ = crate::vfs::filename_rmdir(&Arc::default(), &overlay, "a/b");
                }
            })
        };
        let mut lost = 0;
        for _ in 0..100_000 {
            if crate::vfs::filename_mkdirat(&Arc::default(), &upper, "upper/a/b/made").is_ok() {
                match crate::vfs::vfs_fstatat(&Arc::default(), &upper, "upper/a/b/made", false) {
                    Ok(_) => {
                        let _ =
                            crate::vfs::filename_rmdir(&Arc::default(), &upper, "upper/a/b/made");
                    }
                    Err(_) => lost += 1,
                }
            }
        }
        stop.store(true, Ordering::SeqCst);
        churning.join().expect("the Overlay's calls must not panic");
        assert_eq!(
            lost, 0,
            "vfs::filename_mkdirat in a directory through a Mount of an Overlay's upper Tmpfs, while \
             vfs::filename_rmdir removes that directory through the Overlay, must leave every \
             directory it made standing"
        );
    }

    // The upper has room for the copy's entry but for less than
    // the lower file's bytes, so the copy fails part way.
    // @behavior FS-046
    #[test]
    fn a_copy_up_that_fails_part_way_leaves_nothing_behind() {
        let upper = Arc::new(Tmpfs::new().with_size(Some(2)).with_nr_inodes(Some(8)));
        let (overlay, _) = overlay_on(upper.clone());
        assert_eq!(
            (
                write(&overlay, "dir/file", b"x"),
                on(&upper).symlink_metadata("upper/dir/file"),
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

    /// A Tmpfs answering a lookup of `dir` at any depth with an I/O
    /// failure, though it opens `dir` as any other directory.
    struct LooksUpNoDir;

    impl Hook for LooksUpNoDir {
        fn getattr(
            &self,
            inner: &dyn InodeOperations,
            dentry: &Dentry,
        ) -> Result<Metadata, FilesystemError> {
            if dentry.name() == "dir" {
                return Err(FilesystemError::Other(
                    "dir cannot be looked up".to_string(),
                ));
            }
            inner.getattr(dentry)
        }
    }

    /// A Tmpfs whose `file` is gone after its first lookup, as a host
    /// directory another process changes mid-call.
    #[derive(Default)]
    struct Vanishes {
        seen: AtomicBool,
    }

    impl Hook for Vanishes {
        fn getattr(
            &self,
            inner: &dyn InodeOperations,
            dentry: &Dentry,
        ) -> Result<Metadata, FilesystemError> {
            if dentry.name() == "file" && self.seen.swap(true, Ordering::SeqCst) {
                return Err(FilesystemError::NotFound);
            }
            inner.getattr(dentry)
        }
    }

    /// A Tmpfs storing names in lower case and reaching each by any
    /// case, as a host that folds case does, answering each call by the
    /// folded path from its root.
    #[derive(Clone)]
    struct FoldsCase {
        root: KernPath,
    }

    impl FoldsCase {
        fn new(tmpfs: Tmpfs) -> Self {
            Self { root: at(tmpfs) }
        }
    }

    /// The path `dentry` stands at, folded.
    fn folded(dentry: &Dentry) -> String {
        dentry.path_raw().to_lowercase()
    }

    /// A directory of a `FoldsCase`, open.
    struct FoldedDir(KernPath);

    impl FileOperations for FoldedDir {
        fn iterate_shared(&mut self, dentry: &Dentry) -> Result<Vec<DirEntry>, FilesystemError> {
            match folded(dentry).as_str() {
                "" => self.0.entries(),
                at => self.0.open_dir(at)?.entries(),
            }
        }
    }

    impl InodeOperations for FoldsCase {
        fn getattr(&self, dentry: &Dentry) -> Result<Metadata, FilesystemError> {
            self.root.symlink_metadata(&folded(dentry))
        }

        fn lookup(&self, dentry: &Dentry) -> Option<Result<Lookup, FilesystemError>> {
            let name = dentry.name();
            let lower = name.to_lowercase();
            Some(match self.root.symlink_metadata(&folded(dentry)) {
                Ok(_) if lower != name => Ok(Lookup::Spelled(lower)),
                Ok(found) => Ok(Lookup::Found(Inode::new(&found, Arc::new(self.clone())))),
                Err(FilesystemError::NotFound) => Ok(Lookup::Absent),
                Err(err) => Err(err),
            })
        }

        fn open(
            &self,
            dentry: &Dentry,
            options: &OpenOptions,
        ) -> Option<Result<Box<dyn FileOperations>, FilesystemError>> {
            if options.directory {
                return Some(Ok(Box::new(FoldedDir(self.root.clone()))));
            }
            Some(self.root.open(&folded(dentry), options))
        }

        fn create(
            &self,
            dentry: &Dentry,
            options: &OpenOptions,
        ) -> Option<Result<Box<dyn FileOperations>, FilesystemError>> {
            Some(self.root.open(&folded(dentry), options))
        }

        fn mkdir(&self, dentry: &Dentry) -> Option<Result<(), FilesystemError>> {
            Some(self.root.create_dir(&folded(dentry)))
        }

        fn rmdir(&self, dentry: &Dentry) -> Option<Result<(), FilesystemError>> {
            Some(self.root.remove_dir(&folded(dentry)))
        }

        fn unlink(&self, dentry: &Dentry) -> Option<Result<(), FilesystemError>> {
            Some(self.root.remove_file(&folded(dentry)))
        }

        fn rename(
            &self,
            from: &Dentry,
            _to_dir: &dyn InodeOperations,
            to: &Dentry,
        ) -> Option<Result<(), FilesystemError>> {
            Some(self.root.rename(&folded(from), &self.root, &folded(to)))
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
        let overlay = at(over(Arc::new(Tmpfs::new()), upper.clone()));
        overlay.create_dir("dir").expect("dir must be made");
        overlay.create_dir("dir/inner").expect("inner must be made");
        let dir = overlay.open_dir("dir").expect("dir must open");
        let inner = overlay.open_dir("dir/inner").expect("inner must open");
        overlay
            .rename("dir", &overlay, "moved")
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
        let overlay = at(over(Arc::new(Tmpfs::new()), Arc::new(Tmpfs::new())));
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
        let upper = tmpfs_with("upper/dir/file", "x");
        let overlay = at(over(Arc::new(Tmpfs::new()), upper.clone()));
        let dir = overlay.open_dir("dir").expect("dir must open");
        let outside = on(&upper);
        outside
            .rename("upper/dir", &outside, "upper/moved")
            .expect("the Layer must move it");
        outside
            .create_dir("upper/dir")
            .expect("the Layer must make another");
        overwrite(&outside, "upper/dir/file", b"other");
        assert_eq!(
            read(&dir, "file"),
            b"x".to_vec(),
            "a read through a directory Overlay::open_dir opened, once its Layer moved it away \
             and made another in its place outside the Overlay, must read the directory it \
             opened"
        );
    }

    // Only the directory the view holds open keeps its Layer's host directory
    // through walks past a Bind's bound, so only that reaches it once the host
    // moved it.
    // @behavior FS-152
    #[cfg(unix)]
    #[test]
    fn an_overlay_directory_held_open_keeps_its_bind_layers_directory_past_the_bound() {
        use crate::mount::{Access, Mount};
        use crate::vfs::{self, OpenHow, Path};
        let host = std::env::temp_dir().join(format!("kobako-overlay-held-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&host);
        let deep = host.join(vec!["d"; 100].join("/"));
        std::fs::create_dir_all(&deep).expect("the host tree must be made");
        std::fs::create_dir_all(host.join("held")).expect("the host tree must be made");
        std::fs::write(host.join("held/f"), "x").expect("the host file must be written");
        let lower = Filesystem::new(Arc::new(
            crate::filesystem::Bind::new(&host).expect("the host directory must open"),
        ));
        let overlay = Overlay::new(vec![lower], Some(Filesystem::new(Arc::new(Tmpfs::new()))))
            .expect("the Overlay must stack");
        let mount = Mount::new("/data", Filesystem::new(Arc::new(overlay)), Access::Read)
            .expect("/data is a guest path");
        let root = Path::root(mount.mnt());
        let cred = Arc::default();
        let directory = OpenHow {
            directory: true,
            ..OpenHow::default()
        };
        let Ok(held) = vfs::do_file_open(&cred, &root, "held", &directory) else {
            panic!("the directory must open");
        };
        vfs::vfs_fstatat(&cred, &root, &vec!["d"; 100].join("/"), false)
            .unwrap_or_else(|_| panic!("the deep directory must stand"));
        std::fs::rename(host.join("held"), host.join("moved")).expect("the host must move it");
        let reached = vfs::vfs_fstatat(&cred, held.path(), "f", false);
        let _ = std::fs::remove_dir_all(&host);
        assert!(
            reached.is_ok(),
            "vfs::vfs_fstatat through an Overlay directory held open over a Bind Layer, once a \
             walk passed the bound and the host moved it, must still reach the directory it opened"
        );
    }

    // Only the lower Layer holds `dir` when both handles open it, so the
    // second sees the file only if the copy up through the first reaches
    // it too.
    // @behavior FS-119
    #[test]
    fn a_copy_up_through_one_overlay_directory_is_seen_through_another() {
        let lower: Arc<dyn InodeOperations> = tmpfs_with("dir/a.txt", "a");
        let overlay = at(over(lower, Arc::new(Tmpfs::new())));
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

    // The lower Layer holds no `dir`, so reaching it again by the path it
    // was opened at would miss where it moved.
    // @behavior FS-120
    #[test]
    fn a_write_through_a_moved_overlay_directory_lands_where_it_went() {
        let upper = tmpfs_with("upper/dir/a.txt", "a");
        let overlay = at(over(Arc::new(Tmpfs::new()), upper.clone()));
        let dir = overlay.open_dir("dir").expect("dir must open");
        overlay
            .rename("dir", &overlay, "moved")
            .expect("the directory must move");
        assert_eq!(
            (
                write(&dir, "b.txt", b"b"),
                read(&on(&upper), "upper/moved/b.txt")
            ),
            (Ok(()), b"b".to_vec()),
            "a write through a directory Overlay::open_dir opened, once Overlay::rename moved it \
             over a lower Layer, must land where it moved"
        );
    }

    // @behavior FS-083
    #[test]
    fn an_overlay_spells_a_path_as_the_layer_showing_it_stores_it() {
        let overlay = at(over(
            Arc::new(FoldsCase::new(
                [("private/secret", "x")].into_iter().collect(),
            )),
            Arc::new(Tmpfs::new()),
        ));
        let private = overlay
            .open_dir("PRIVATE")
            .expect("the directory must open in any case");
        assert_eq!(
            (
                overlay.lookup("PRIVATE"),
                private.lookup("SECRET"),
                private.lookup("New")
            ),
            (
                Ok("private".to_string()),
                Ok("secret".to_string()),
                Ok("New".to_string())
            ),
            "a name asked of Overlay::lookup in another case, shown by a Layer folding case, \
             must come back as that Layer stores it, a new name kept as given"
        );
    }

    // A directory only one Layer shows has nothing to merge or hide, so the
    // walk opens it as that Layer does, as overlayfs leaves it unmerged.
    // @behavior FS-122
    #[test]
    fn a_directory_one_layer_shows_is_walked_without_a_lookup() {
        let only: Arc<dyn InodeOperations> = tmpfs_with("top/dir/file", "only");
        let overlay = at(over(
            Arc::new(Wrap::new(only, Arc::new(LooksUpNoDir))),
            Arc::new(Tmpfs::new()),
        ));
        assert_eq!(
            read(&overlay, "top/dir/file"),
            b"only",
            "a file read through Overlay::open beneath a directory only the lower Layer shows, \
             that Layer answering no lookup of the directory, must read its content"
        );
    }

    // @behavior FS-080
    #[test]
    fn a_file_gone_during_a_rename_answers_not_found() {
        let tmpfs: Tmpfs = [("file", "lower")].into_iter().collect();
        let lower = Wrap::new(Arc::new(tmpfs), Arc::new(Vanishes::default()));
        let overlay = at(over(Arc::new(lower), Arc::new(Tmpfs::new())));
        assert_eq!(
            overlay.rename("file", &overlay, "moved"),
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
        let overlay = at(over(
            tmpfs_with("dir", "lower"),
            tmpfs_with("upper/dir/sub/file", "upper"),
        ));
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
            .rename("dir/file", &overlay, "dir/moved")
            .expect("the rename must land");
        overwrite(&overlay, "dir/moved", b"moved");
        assert_eq!(
            read_from(reader.as_mut()),
            b"moved".to_vec(),
            "a file Overlay::open handed out to read before Overlay::rename moved it must read \
             what the moved copy holds"
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
            .rename("dir/file", &overlay, "dir/moved")
            .expect("the rename must land");
        assert_eq!(
            read_from(reader.as_mut()),
            b"upper".to_vec(),
            "a file Overlay::open handed out to read must keep reading what it held once \
             Overlay::remove_file took it away, whatever later stands at its path"
        );
    }

    /// A Tmpfs whose first open sleeps a while, flagging that it began.
    #[derive(Default)]
    struct SleepsOnOpen {
        opening: AtomicBool,
    }

    impl Hook for SleepsOnOpen {
        fn open(
            &self,
            inner: &dyn InodeOperations,
            dentry: &Dentry,
            options: &OpenOptions,
        ) -> Option<Result<Box<dyn FileOperations>, FilesystemError>> {
            if !self.opening.swap(true, Ordering::SeqCst) {
                thread::sleep(Duration::from_millis(100));
            }
            inner.open(dentry, options)
        }
    }

    // The first copy up holds the file's lock while its lower open sleeps,
    // so the second, of the same file, finds the lock held and waits.
    // @behavior FS-039
    #[test]
    fn a_contended_copy_up_waits_through_the_registered_blocking() {
        let counts = crate::sched::testing::count_waits();
        let sleeps = Arc::new(SleepsOnOpen::default());
        let lower = Wrap::new(tmpfs_with("note", "lower"), sleeps.clone());
        let overlay = over(Arc::new(lower), Arc::new(Tmpfs::new()));
        let first = at(overlay);
        let second = first.clone();
        let copying = thread::spawn(move || write(&first, "note", b"first"));
        while !sleeps.opening.load(Ordering::SeqCst) {
            thread::sleep(Duration::from_millis(1));
        }
        let waited = write(&second, "note", b"later");
        assert_eq!(
            (
                copying.join().expect("the first copy up must not panic"),
                waited,
                counts.load(Ordering::SeqCst) > 0
            ),
            (Ok(()), Ok(()), true),
            "a write through an Overlay copying up a file another thread is copying up must wait \
             through the Blocking given to sched::register_blocking, then land"
        );
    }

    // Copying up makes the directory in the upper, whose own identity
    // differs, so only reporting the lower Layer's keeps it.
    // @behavior FS-072
    #[test]
    fn an_overlay_keeps_a_directorys_identity_when_a_write_copies_it_up() {
        let lower: Arc<dyn InodeOperations> =
            Arc::new([("dir/a.txt", "a")].into_iter().collect::<Tmpfs>());
        let overlay = at(over(lower, Arc::new(Tmpfs::new())));
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
            "a directory of a lower Layer copied up by a write through an Overlay's open must \
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
        let overlay = at(over(lower.clone(), upper.clone()));
        std::thread::sleep(std::time::Duration::from_millis(10));
        write(&overlay, "dir/b.txt", b"b").expect("the upper must take a write");
        let merged = overlay
            .symlink_metadata("dir")
            .expect("the directory must stand");
        let modified = |layer: &Arc<Tmpfs>, path: &str| {
            on(layer)
                .symlink_metadata(path)
                .expect("a Layer holds it")
                .modified
        };
        assert_eq!(
            (
                merged.nlink,
                merged.modified == modified(&upper, "upper/dir"),
                merged.modified == modified(&lower, "dir"),
            ),
            (1, true, false),
            "Overlay::metadata of a directory two Layers hold, written last in the upper, must \
             count one name and report the upper Layer's times"
        );
    }

    // @behavior FS-073
    #[test]
    fn an_overlay_names_a_directory_alike_whether_a_link_at_its_end_is_followed() {
        let overlay = at(over(
            tmpfs_with("dir/lower", "x"),
            tmpfs_with("upper/dir/upper", "x"),
        ));
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
