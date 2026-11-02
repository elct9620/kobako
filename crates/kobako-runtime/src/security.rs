//! The security modules an Invocation registers: kobako's own rules, and the
//! Host App's Policy for each Mount that has one. Linux has neither, so they
//! answer through the hooks it leaves for such modules, and the VFS never
//! learns what they decide.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use crate::mount::{Mount, Policy};
use crate::vfs::{
    d_revalidate, Access, Cred, Dentry, FileType, FilesystemError, Path, SecurityOperations,
    SecurityPath, VfsMount, PATH_MAX,
};

/// How many times `revalidate_path` asks from the root before a move landing in
/// every pass answers EAGAIN, as a scoped walk's `..` answers a move it met.
const MAX_PASSES: usize = 4;

/// Whether each name from the root down to `path` still names what the runtime
/// recorded, as gVisor's `revalidatePath` walks one. Linux asks only names a walk
/// passes through, so a `WalkGuard` asks this of where a call starts. A name moved
/// while it was asked is asked again from the root, where it went, at most
/// `MAX_PASSES` times in all, so moves landing without pause cannot hold the
/// caller.
fn revalidate_path(path: &Path) -> Result<bool, FilesystemError> {
    'again: for _ in 0..MAX_PASSES {
        let mut chain = vec![path.dentry().clone()];
        while let Some(above) = chain.last().and_then(|at| at.parent()) {
            chain.push(above);
        }
        for pair in chain.windows(2).rev() {
            let [name, above] = pair else { continue };
            let _held = above.inode_lock_shared()?;
            if !name.parent().is_some_and(|now| Arc::ptr_eq(&now, above)) {
                continue 'again;
            }
            if !d_revalidate(path.mnt().sb(), above, name)? {
                return Ok(false);
            }
        }
        return Ok(true);
    }
    Err(FilesystemError::WouldBlock)
}

/// What every Invocation holds to, whatever its Mounts allow.
struct KobakoSecurity;

impl SecurityOperations for KobakoSecurity {
    /// Only a file or a directory opens: the runtime provides no peer for a
    /// FIFO or a socket, and a device would act on being opened.
    fn file_open(
        &self,
        _path: SecurityPath<'_>,
        kind: FileType,
        _access: Access,
    ) -> Result<(), FilesystemError> {
        match kind {
            FileType::File | FileType::Directory => Ok(()),
            _ => Err(FilesystemError::PermissionDenied),
        }
    }

    /// A guest never reads where a link points.
    fn inode_readlink(&self) -> Result<(), FilesystemError> {
        Err(FilesystemError::Unsupported)
    }
}

/// A Mount's Policy, answering only for that Mount.
struct MountPolicy {
    mnt: Arc<VfsMount>,
    at: String,
    policy: Arc<dyn Policy>,
}

impl MountPolicy {
    fn guards(&self, mnt: &Arc<VfsMount>) -> bool {
        Arc::ptr_eq(&self.mnt, mnt)
    }

    /// The absolute guest path `path`, relative to the Mount's root, stands
    /// for.
    fn guest_path(&self, path: &str) -> String {
        match (self.at.as_str(), path) {
            (at, "") => at.to_string(),
            ("/", path) => format!("/{path}"),
            (at, path) => format!("{at}/{path}"),
        }
    }

    /// Judge `path` as AppArmor's `aa_path_perm` judges a whole path once, each
    /// hook below reaching it as AppArmor's do. The Policy judges the path as the
    /// Filesystem stores its names, so a host folding case or Unicode form cannot
    /// hand it one spelling of an entry it refuses by another. One a Policy could
    /// not be handed within `PATH_MAX` is refused as too long, as AppArmor's
    /// `aa_path_name` refuses one past its buffer.
    fn path_perm(&self, path: SecurityPath<'_>, access: Access) -> Result<(), FilesystemError> {
        if !self.guards(path.mnt) {
            return Ok(());
        }
        let path = self.guest_path(&(path.name)());
        if path.len() >= PATH_MAX {
            return Err(FilesystemError::InvalidFilename);
        }
        match self.policy.allows(&path, access) {
            true => Ok(()),
            false => Err(FilesystemError::PermissionDenied),
        }
    }
}

impl SecurityOperations for MountPolicy {
    fn path_mknod(&self, path: SecurityPath<'_>) -> Result<(), FilesystemError> {
        self.path_perm(path, Access::ReadWrite)
    }

    fn path_mkdir(&self, path: SecurityPath<'_>) -> Result<(), FilesystemError> {
        self.path_perm(path, Access::ReadWrite)
    }

    fn path_rmdir(&self, path: SecurityPath<'_>) -> Result<(), FilesystemError> {
        self.path_perm(path, Access::ReadWrite)
    }

    fn path_unlink(&self, path: SecurityPath<'_>) -> Result<(), FilesystemError> {
        self.path_perm(path, Access::ReadWrite)
    }

    fn path_symlink(&self, path: SecurityPath<'_>) -> Result<(), FilesystemError> {
        self.path_perm(path, Access::ReadWrite)
    }

    fn path_link(
        &self,
        old: SecurityPath<'_>,
        new: SecurityPath<'_>,
    ) -> Result<(), FilesystemError> {
        self.path_perm(old, Access::ReadWrite)?;
        self.path_perm(new, Access::ReadWrite)
    }

    fn file_open(
        &self,
        path: SecurityPath<'_>,
        _kind: FileType,
        access: Access,
    ) -> Result<(), FilesystemError> {
        self.path_perm(path, access)
    }

    fn inode_setattr(&self, path: SecurityPath<'_>) -> Result<(), FilesystemError> {
        self.path_perm(path, Access::ReadWrite)
    }

    /// Both paths are judged. Everything beneath a directory would take paths the
    /// Policy never judged, and a Policy answers for one path, not a tree, so its
    /// move is refused as one across devices and the guest copies entry by entry
    /// instead, as Landlock answers a move it cannot show keeps each right beneath.
    /// A move onto its own name takes no new path.
    fn path_rename(
        &self,
        old: SecurityPath<'_>,
        new: SecurityPath<'_>,
        (old_dentry, new_dentry): (&Arc<Dentry>, &Arc<Dentry>),
    ) -> Result<(), FilesystemError> {
        self.path_perm(old, Access::ReadWrite)?;
        self.path_perm(new, Access::ReadWrite)?;
        let moves_dir =
            old_dentry.kind() == Ok(FileType::Directory) && !Arc::ptr_eq(old_dentry, new_dentry);
        match moves_dir && self.guards(old.mnt) {
            true => Err(FilesystemError::CrossesDevices),
            false => Ok(()),
        }
    }
}

/// What keeps a Policy judging the path a descriptor stands at, kobako's own
/// guarantee: Linux asks nothing of the dentry a walk starts from, so nothing in
/// its VFS can ask this, and a Driver asks it wherever a descriptor becomes where
/// a call starts.
pub struct WalkGuard {
    /// The mounts a Policy judges; no other holds anything to keep true.
    guarded: Vec<Arc<VfsMount>>,
}

impl WalkGuard {
    /// What keeps each Policy over `mounts` judging the path it means to.
    pub fn new<'a>(mounts: impl IntoIterator<Item = &'a Mount>) -> Self {
        let guarded = mounts
            .into_iter()
            .filter(|mount| mount.policy().is_some())
            .map(|mount| mount.mnt().clone())
            .collect();
        Self { guarded }
    }

    /// Refuse a call through a descriptor unless its dentry still names what the
    /// runtime opened there, so the Policy judges the path it stands at. A dentry
    /// dropped or removed is refused as Linux's AppArmor refuses an unlinked one,
    /// and one whose identity the runtime never learned cannot be told from another,
    /// so it is refused too. This keeps a host or another Filesystem instance moving
    /// a directory behind the runtime's back from having a Policy judge a path it no
    /// longer stands at.
    pub fn check(&self, path: &Path) -> Result<(), FilesystemError> {
        if !self.guarded.iter().any(|mnt| Arc::ptr_eq(mnt, path.mnt())) {
            return Ok(());
        }
        let dentry = path.dentry();
        if dentry.parent().is_none() {
            return Ok(());
        }
        let told_apart = dentry
            .inode()
            .is_some_and(|inode| inode.identity().is_some());
        if path.mnt().sb().dentries().unlinked(dentry) || !told_apart || !revalidate_path(path)? {
            return Err(FilesystemError::NotFound);
        }
        Ok(())
    }
}

/// How many bytes a Filesystem holds on one caller's behalf between calls, so a
/// guest's descriptors cannot multiply a directory past what it was allotted, as
/// `__vm_enough_memory` weighs what `vm_committed_as` counts.
struct MemoryLimit {
    limit: usize,
    used: AtomicUsize,
}

impl SecurityOperations for MemoryLimit {
    fn vm_enough_memory(&self, bytes: usize) -> Result<(), FilesystemError> {
        self.used
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |used| {
                used.checked_add(bytes).filter(|&total| total <= self.limit)
            })
            .map(|_| ())
            .map_err(|_| FilesystemError::OutOfMemory)
    }

    fn vm_unacct_memory(&self, bytes: usize) {
        self.used.fetch_sub(bytes, Ordering::Relaxed);
    }
}

/// The Cred an Invocation over `mounts` runs with, its Filesystems holding at most
/// `memory_limit` bytes on its behalf, or without bound for `None`.
pub fn prepare_creds_within<'a>(
    mounts: impl IntoIterator<Item = &'a Mount>,
    memory_limit: Option<usize>,
) -> Arc<Cred> {
    let mut security = modules_for(mounts);
    if let Some(limit) = memory_limit {
        security.push(Arc::new(MemoryLimit {
            limit,
            used: AtomicUsize::new(0),
        }));
    }
    Cred::new(security)
}

/// The Cred an Invocation over `mounts` runs with, as `prepare_creds` makes
/// one for a task: kobako's own rules, then each Mount's Policy.
pub fn prepare_creds<'a>(mounts: impl IntoIterator<Item = &'a Mount>) -> Arc<Cred> {
    Cred::new(modules_for(mounts))
}

fn modules_for<'a>(
    mounts: impl IntoIterator<Item = &'a Mount>,
) -> Vec<Arc<dyn SecurityOperations>> {
    let mut security: Vec<Arc<dyn SecurityOperations>> = vec![Arc::new(KobakoSecurity)];
    for mount in mounts {
        if let Some(policy) = mount.policy() {
            security.push(Arc::new(MountPolicy {
                mnt: mount.mnt().clone(),
                at: mount.at().to_string(),
                policy: policy.clone(),
            }));
        }
    }
    security
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filesystem::wrap::{Hook, Wrap};
    use crate::filesystem::{DentryOperations, Filesystem, InodeOperations, Tmpfs};

    /// A Policy allowing everything, counting how often it is asked.
    #[derive(Default)]
    struct Counts(std::sync::atomic::AtomicUsize);

    impl Policy for Counts {
        fn allows(&self, _path: &str, _access: Access) -> bool {
            self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            true
        }
    }

    /// What making an entry answers through a directory moved past
    /// `PATH_MAX`, beneath a mount `prepare` finishes.
    fn made_past_the_limit(prepare: impl FnOnce(Mount) -> Mount) -> Result<(), FilesystemError> {
        let (deep, seed) = (vec!["a"; 2000].join("/"), "x");
        let tmpfs: Tmpfs = [
            (format!("c0/{deep}/file"), seed),
            (format!("c1/{deep}/file"), seed),
        ]
        .into_iter()
        .collect();
        let mount = Mount::new(
            "/data",
            Filesystem::new(Arc::new(tmpfs)).expect("the instance must stand"),
            Access::ReadWrite,
        )
        .expect("/data is a guest path");
        let mount = prepare(mount);
        let cred = prepare_creds([&mount]);
        let root = Path::root(mount.mnt());
        let how = crate::vfs::OpenHow {
            directory: true,
            ..Default::default()
        };
        let held = crate::vfs::do_file_open(&cred, &root, &format!("c0/{deep}"), &how)
            .unwrap_or_else(|_| panic!("the directory must open"));
        crate::vfs::filename_renameat2(
            &Arc::default(),
            &root,
            "c0",
            &root,
            &format!("c1/{deep}/z"),
        )
        .expect("the move must land");
        crate::vfs::filename_mkdirat(&cred, held.path(), "made")
    }

    // Linux's walk sets no limit on where a directory stands, so a walk
    // without a Policy has no reason to.
    // @behavior RT-203
    #[test]
    fn a_path_past_the_limit_is_made_where_no_policy_needs_it() {
        assert_eq!(
            made_past_the_limit(|mount| mount),
            Ok(()),
            "vfs::filename_mkdirat through a directory moved past PATH_MAX, beneath a Mount with \
             no Policy, must make the entry"
        );
    }

    // @behavior RT-204
    #[test]
    fn a_path_a_policy_cannot_be_handed_is_too_long() {
        let policy = Arc::new(Counts::default());
        let asked = policy.clone();
        let made = made_past_the_limit(move |mount| mount.with_policy(policy));
        let asked_past_the_limit = asked.0.load(std::sync::atomic::Ordering::SeqCst);
        assert_eq!(
            (made, asked_past_the_limit),
            (Err(FilesystemError::InvalidFilename), 1),
            "vfs::filename_mkdirat through a directory moved past PATH_MAX, beneath a Policy, \
             must answer ENAMETOOLONG, the Policy asked only of the open before the move"
        );
    }

    /// Holds the first ask of `revalidate` about `d` once it reached the
    /// Filesystem, so a move of `d` racing it either lands first or waits for
    /// the answer.
    struct PausesRevalidate(std::sync::Mutex<Option<std::sync::mpsc::Sender<()>>>);

    impl Hook for PausesRevalidate {
        fn revalidate(
            &self,
            ops: &dyn DentryOperations,
            dir: &dyn InodeOperations,
            dentry: &Dentry,
        ) -> Result<bool, FilesystemError> {
            let reached = match dentry.name() == "d" {
                true => self.0.lock().expect("the sender").take(),
                false => None,
            };
            if let Some(reached) = reached {
                reached.send(()).expect("the test waits for the ask");
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            ops.revalidate(dir, dentry)
        }
    }

    /// One Filesystem with a Mount under an allow-all Policy holding `p/d` open,
    /// and a Mount with none to move it through.
    fn held_beneath_a_policy(
        filesystem: Arc<Filesystem>,
    ) -> (Arc<Cred>, crate::vfs::File, Mount, WalkGuard) {
        let guarded = Mount::new("/guarded", filesystem.clone(), Access::ReadWrite)
            .expect("/guarded is a guest path")
            .with_policy(Arc::new(Counts::default()));
        let plain =
            Mount::new("/plain", filesystem, Access::ReadWrite).expect("/plain is a guest path");
        let cred = prepare_creds([&guarded]);
        let how = crate::vfs::OpenHow {
            directory: true,
            ..Default::default()
        };
        let held = crate::vfs::do_file_open(&cred, &Path::root(guarded.mnt()), "p/d", &how)
            .unwrap_or_else(|_| panic!("the directory must open"));
        (cred, held, plain, WalkGuard::new([&guarded]))
    }

    // The ask of `revalidate` and a move through the other Mount meet only if
    // the walk holds no lock: then the Filesystem has moved the entry when it is
    // asked, the runtime lets go of a dentry still in use, and the move finds
    // nothing to carry along.
    // @behavior RT-234
    #[test]
    fn a_directory_moved_while_a_call_starts_from_it_stays_reachable_beneath_a_policy() {
        let tmpfs: Tmpfs = [("p/d/f", "x"), ("q/keep", "x")].into_iter().collect();
        let (reached, asked) = std::sync::mpsc::channel();
        let hook = Arc::new(PausesRevalidate(std::sync::Mutex::new(None)));
        let filesystem = Filesystem::new(Arc::new(Wrap::new(Arc::new(tmpfs), hook.clone())))
            .expect("the instance must stand");
        let (cred, held, plain, guard) = held_beneath_a_policy(filesystem);
        *hook.0.lock().expect("the sender") = Some(reached);
        let start = held.path().clone();
        let first = std::thread::scope(|scope| {
            let first = scope.spawn(|| guard.check(&start));
            asked
                .recv_timeout(crate::sched::testing::MAX_WAIT)
                .expect("the call must ask revalidate");
            let root = Path::root(plain.mnt());
            crate::vfs::filename_renameat2(&prepare_creds([]), &root, "p/d", &root, "q/d")
                .expect("the move must land");
            crate::sched::testing::join_scoped("the call", first)
        });
        assert!(
            first.is_ok()
                && guard.check(held.path()).is_ok()
                && crate::vfs::vfs_fstatat(&cred, held.path(), "f", false).is_ok()
                && held.path().dentry().path_raw() == "q/d",
            "WalkGuard::check of a directory moved by another Mount while revalidate is asked, \
             beneath a Policy, must let a call through both then and afterwards, at q/d"
        );
    }

    /// Moves `d` between `p` and `q` through `plain` each time the guarding thread
    /// asks either whether it still stands, so every pass from the root meets a
    /// move, until it has moved `MOVES` times.
    struct MovesEachPass {
        guarding: std::sync::OnceLock<std::thread::ThreadId>,
        plain: std::sync::OnceLock<Path>,
        moved: std::sync::atomic::AtomicUsize,
    }

    impl MovesEachPass {
        const MOVES: usize = 100;
    }

    impl Hook for MovesEachPass {
        fn revalidate(
            &self,
            ops: &dyn crate::vfs::DentryOperations,
            dir: &dyn crate::filesystem::InodeOperations,
            dentry: &Dentry,
        ) -> Result<bool, FilesystemError> {
            use std::sync::atomic::Ordering;
            let guarding = self.guarding.get() == Some(&std::thread::current().id());
            let from = dentry.name();
            if guarding
                && (from == "p" || from == "q")
                && self.moved.load(Ordering::SeqCst) < Self::MOVES
            {
                let to = if from == "p" { "q" } else { "p" };
                let root = self.plain.get().expect("the plain Mount stands").clone();
                std::thread::spawn(move || {
                    crate::vfs::filename_renameat2(
                        &prepare_creds([]),
                        &root,
                        &format!("{from}/d"),
                        &root,
                        &format!("{to}/d"),
                    )
                })
                .join()
                .expect("the move must not panic")
                .expect("the move must land");
                self.moved.fetch_add(1, Ordering::SeqCst);
            }
            ops.revalidate(dir, dentry)
        }
    }

    // Each pass from the root meets a move another Mount makes, so only a bound on
    // the passes lets the call answer while the moves go on.
    // @behavior RT-266
    #[test]
    fn a_start_moved_in_every_pass_answers_eagain_beneath_a_policy() {
        use std::sync::atomic::Ordering;

        let tmpfs: Tmpfs = [("p/d/f", "x"), ("q/keep", "x")].into_iter().collect();
        let hook = Arc::new(MovesEachPass {
            guarding: std::sync::OnceLock::new(),
            plain: std::sync::OnceLock::new(),
            moved: std::sync::atomic::AtomicUsize::new(0),
        });
        let filesystem = Filesystem::new(Arc::new(Wrap::new(Arc::new(tmpfs), hook.clone())))
            .expect("the instance must stand");
        let (_cred, held, plain, guard) = held_beneath_a_policy(filesystem);
        hook.plain.set(Path::root(plain.mnt())).ok();
        hook.guarding.set(std::thread::current().id()).ok();
        assert_eq!(
            (guard.check(held.path()), hook.moved.load(Ordering::SeqCst)),
            (Err(FilesystemError::WouldBlock), MAX_PASSES),
            "WalkGuard::check of a directory another Mount moves in every pass, beneath a \
             Policy, must answer EAGAIN once its passes are used up"
        );
    }

    mod stress {
        use std::sync::atomic::{AtomicBool, Ordering};

        use super::*;

        // Nothing refuses a directory that stands, so a single refusal while it
        // moves between two names is a dentry the walk let go of.
        // @behavior RT-234
        #[test]
        #[ignore = "a stress witness; run by rake crates:stress"]
        fn a_directory_moving_while_calls_start_from_it_stays_reachable_beneath_a_policy() {
            let tmpfs: Tmpfs = [("p/d/f", "x"), ("q/keep", "x")].into_iter().collect();
            let (cred, held, plain, guard) = held_beneath_a_policy(
                Filesystem::new(Arc::new(tmpfs)).expect("the instance must stand"),
            );
            let stop = AtomicBool::new(false);
            let start = held.path().clone();
            let refused = std::thread::scope(|scope| {
                scope.spawn(|| {
                    let (root, cred) = (Path::root(plain.mnt()), prepare_creds([]));
                    while !stop.load(Ordering::SeqCst) {
                        for (from, to) in [("p/d", "q/d"), ("q/d", "p/d")] {
                            crate::vfs::filename_renameat2(&cred, &root, from, &root, to)
                                .expect("the move must land");
                        }
                    }
                });
                let refused = (0..200_000)
                    .filter(|_| {
                        guard.check(&start).is_err()
                            || crate::vfs::vfs_fstatat(&cred, &start, "f", false).is_err()
                    })
                    .count();
                stop.store(true, Ordering::SeqCst);
                refused
            });
            assert_eq!(
                refused, 0,
                "vfs::vfs_fstatat through a directory another Mount keeps moving, beneath a \
                 Policy, must always find its file"
            );
        }
    }

    // A Bind refuses these itself, so only asking the rule directly shows it
    // holds for a Filesystem that would open them.
    // @behavior RT-197
    #[test]
    fn only_a_file_or_a_directory_opens() {
        let mount = Mount::new(
            "/data",
            crate::filesystem::Filesystem::new(Arc::new(crate::filesystem::Tmpfs::new()))
                .expect("the instance must stand"),
            Access::Read,
        )
        .expect("/data is a guest path");
        let judged = |kind| {
            let path = SecurityPath {
                mnt: mount.mnt(),
                name: &String::new,
            };
            KobakoSecurity.file_open(path, kind, Access::Read).is_ok()
        };
        assert_eq!(
            [
                FileType::File,
                FileType::Directory,
                FileType::Symlink,
                FileType::Fifo,
                FileType::Socket,
                FileType::BlockDevice,
                FileType::CharDevice,
                FileType::Unknown,
            ]
            .map(judged),
            [true, true, false, false, false, false, false, false],
            "KobakoSecurity::file_open must pass a file and a directory, and refuse every other \
             kind"
        );
    }
}
