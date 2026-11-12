//! The security modules an Invocation registers: kobako's own rules, the Host
//! App's Policy for each Mount that has one, and each network entry's connect
//! policy. Linux has neither, so they
//! answer through the hooks it leaves for such modules, and the VFS never
//! learns what they decide.

use std::net::SocketAddr;
use std::sync::Arc;

use crate::grant::{Grant, Policy};
use crate::memcg::MemoryBudget;
use crate::mount::{Access, Mount, MountRequest};
use crate::network::Connection;
use crate::vfs::{
    d_revalidate, Cred, Dentry, FMode, FileSecurity, FileType, MntNamespace, OpenHow, Path,
    SecurityOperations, SecurityPath, VfsMount, PATH_MAX,
};
use crate::Errno;

/// What a Mount's Policy allowed a file as it opened, kept on the open file, as
/// Landlock's file blob keeps `allowed_access`.
struct AllowedAccess(Access);

/// Whether each name from the root down to `path` still names what the runtime
/// recorded, as gVisor's `revalidatePath` walks one. Linux asks only names a walk
/// passes through, so a `WalkGuard` asks this of where a call starts. A name moved
/// while it was asked answers EAGAIN, as a scoped walk's `..` answers a move it
/// met, and the caller makes the call again.
fn revalidate_path(path: &Path) -> Result<bool, Errno> {
    let mut chain = vec![path.dentry().clone()];
    while let Some(at) = chain.last().filter(|at| !at.is_root()) {
        let above = at.parent();
        chain.push(above);
    }
    for pair in chain.windows(2).rev() {
        let [name, above] = pair else { continue };
        let _held = above.inode_lock_shared()?;
        if !Arc::ptr_eq(&name.parent(), above) {
            return Err(Errno::WouldBlock);
        }
        if !d_revalidate(path.mnt().sb(), above, name)? {
            return Ok(false);
        }
    }
    Ok(true)
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
        _mode: FMode,
        _how: &OpenHow,
    ) -> Result<FileSecurity, Errno> {
        match kind {
            FileType::RegFile | FileType::Dir => Ok(None),
            _ => Err(Errno::PermissionDenied),
        }
    }

    /// A guest never reads where a link points.
    fn inode_readlink(&self, _dentry: &Dentry) -> Result<(), Errno> {
        Err(Errno::Unsupported)
    }

    /// A guest only reaches out, so it binds the unspecified address and port
    /// 0 alone, as Landlock's `hook_socket_bind` refuses a bind no
    /// `BIND_TCP` right allows.
    fn socket_bind(&self, address: SocketAddr) -> Result<(), Errno> {
        match address.ip().is_unspecified() && address.port() == 0 {
            true => Ok(()),
            false => Err(Errno::PermissionDenied),
        }
    }
}

/// A Mount's Policy, answering only for that Mount.
struct MountPolicy {
    mnt: Arc<VfsMount>,
    at: String,
    policy: Arc<dyn for<'a> Policy<MountRequest<'a>>>,
}

impl MountPolicy {
    fn guards(&self, mnt: &Arc<VfsMount>) -> bool {
        Arc::ptr_eq(&self.mnt, mnt)
    }

    /// The absolute guest path `path`, from the Mount's root, stands for.
    fn guest_path(&self, path: &str) -> String {
        match (self.at.as_str(), path) {
            (at, "/") => at.to_string(),
            ("/", path) => path.to_string(),
            (at, path) => format!("{at}{path}"),
        }
    }

    /// Judge `path` as AppArmor's `aa_path_perm` judges a whole path once, each
    /// hook below reaching it as AppArmor's do. The Policy judges the path as the
    /// Filesystem stores its names, so a host folding case or Unicode form cannot
    /// hand it one spelling of an entry it refuses by another. One a Policy could
    /// not be handed within `PATH_MAX` is refused as too long, as AppArmor's
    /// `aa_path_name` refuses one past its buffer.
    fn path_perm(&self, path: SecurityPath<'_>, access: Access) -> Result<(), Errno> {
        if !self.guards(path.mnt) {
            return Ok(());
        }
        let path = self.guest_path(&(path.name)());
        if path.len() >= PATH_MAX {
            return Err(Errno::InvalidFilename);
        }
        match self.policy.allows(&MountRequest {
            path: &path,
            access,
        }) {
            true => Ok(()),
            false => Err(Errno::PermissionDenied),
        }
    }
}

impl SecurityOperations for MountPolicy {
    fn path_mknod(&self, path: SecurityPath<'_>) -> Result<(), Errno> {
        self.path_perm(path, Access::ReadWrite)
    }

    fn path_mkdir(&self, path: SecurityPath<'_>) -> Result<(), Errno> {
        self.path_perm(path, Access::ReadWrite)
    }

    fn path_rmdir(&self, path: SecurityPath<'_>) -> Result<(), Errno> {
        self.path_perm(path, Access::ReadWrite)
    }

    fn path_unlink(&self, path: SecurityPath<'_>) -> Result<(), Errno> {
        self.path_perm(path, Access::ReadWrite)
    }

    fn path_symlink(&self, path: SecurityPath<'_>) -> Result<(), Errno> {
        self.path_perm(path, Access::ReadWrite)
    }

    fn path_link(&self, old: SecurityPath<'_>, new: SecurityPath<'_>) -> Result<(), Errno> {
        self.path_perm(old, Access::ReadWrite)?;
        self.path_perm(new, Access::ReadWrite)
    }

    /// Asked once for each open, for what the open may change, and kept on the
    /// file, as Landlock keeps the rights it allowed. A file the open made was
    /// judged as a change already, through `path_mknod`.
    fn file_open(
        &self,
        path: SecurityPath<'_>,
        _kind: FileType,
        mode: FMode,
        how: &OpenHow,
    ) -> Result<FileSecurity, Errno> {
        if !self.guards(path.mnt) {
            return Ok(None);
        }
        let created = mode.contains(FMode::CREATED);
        let access = match created || mode.contains(FMode::WRITE) || how.truncate {
            true => Access::ReadWrite,
            false => Access::Read,
        };
        if !created {
            self.path_perm(path, access)?;
        }
        Ok(Some(Arc::new(AllowedAccess(access))))
    }

    /// A truncate follows what the Policy allowed as the file opened, wherever
    /// the file stands now, as Landlock's `hook_file_truncate` reads the file's
    /// rights.
    fn file_truncate(&self, kept: &FileSecurity) -> Result<(), Errno> {
        match kept
            .as_deref()
            .and_then(|kept| kept.downcast_ref::<AllowedAccess>())
        {
            Some(AllowedAccess(Access::Read)) => Err(Errno::PermissionDenied),
            _ => Ok(()),
        }
    }

    fn path_utimes(&self, path: SecurityPath<'_>) -> Result<(), Errno> {
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
    ) -> Result<(), Errno> {
        self.path_perm(old, Access::ReadWrite)?;
        self.path_perm(new, Access::ReadWrite)?;
        let moves_dir =
            old_dentry.kind() == Ok(FileType::Dir) && !Arc::ptr_eq(old_dentry, new_dentry);
        match moves_dir && self.guards(old.mnt) {
            true => Err(Errno::CrossesDevices),
            false => Ok(()),
        }
    }
}

/// What keeps a Policy judging the path a descriptor stands at, kobako's own
/// guarantee: Linux asks nothing of the dentry a walk starts from, so nothing in
/// its VFS can ask this, and a Wasm Driver asks it wherever a descriptor becomes where
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
    pub fn check(&self, path: &Path) -> Result<(), Errno> {
        if !self.guarded.iter().any(|mnt| Arc::ptr_eq(mnt, path.mnt())) {
            return Ok(());
        }
        let dentry = path.dentry();
        if dentry.is_root() {
            return Ok(());
        }
        let told_apart = dentry
            .inode()
            .is_some_and(|inode| inode.identity().is_some());
        if path.mnt().sb().dentries().unlinked(dentry) || !told_apart || !revalidate_path(path)? {
            return Err(Errno::NotFound);
        }
        Ok(())
    }
}

/// The Cred an Invocation opens its Mounts' roots with, charging what the host
/// holds on its behalf to one budget of `memory_limit` bytes, or without bound
/// for `None`: kobako's
/// own rules, then each of `modules` in order, before any Mount's Policy applies,
/// as a Linux task opens the files it keeps before `landlock_restrict_self`.
/// The modules are those the Wasm Driver was built with, as Linux stacks the LSMs
/// `lsm=` names at boot after the capability module.
pub fn prepare_creds_within(
    modules: &[Arc<dyn SecurityOperations>],
    memory_limit: Option<usize>,
) -> Arc<Cred> {
    let mut security: Vec<Arc<dyn SecurityOperations>> = vec![Arc::new(KobakoSecurity)];
    security.extend(modules.iter().cloned());
    Cred::within(security, MemoryBudget::new(memory_limit))
}

/// An entry's connect policy, judging what the guest named, as one more LSM
/// answers `security_socket_connect`.
struct EntryPolicy(Arc<dyn for<'a> Policy<Connection<'a>>>);

impl SecurityOperations for EntryPolicy {
    fn socket_connect(&self, connection: &Connection<'_>) -> Result<(), Errno> {
        match self.0.allows(connection) {
            true => Ok(()),
            false => Err(Errno::PermissionDenied),
        }
    }
}

/// `cred` with each Mount's Policy and each network entry's connect policy
/// added, the modules it held and the memory held on its behalf shared, so
/// what either Cred charges counts against one limit, as
/// `landlock_restrict_self` adds a domain ruling both paths and ports. Its
/// mount namespace holds the Mounts among `grants`, each beneath the one whose
/// path holds it, so a walk reaching one by any spelling steps onto it.
pub fn restrict_self<'a>(cred: &Cred, grants: impl IntoIterator<Item = &'a Grant>) -> Arc<Cred> {
    let grants: Vec<&Grant> = grants.into_iter().collect();
    let mounts = grants.iter().filter_map(|grant| match grant {
        Grant::Mount(mount) => Some((mount.at(), mount.mnt())),
        Grant::Network(_) => None,
    });
    cred.with_modules(
        policies_of(grants.iter().copied()),
        MntNamespace::new(mounts),
    )
}

fn policies_of<'a>(
    grants: impl IntoIterator<Item = &'a Grant>,
) -> Vec<Arc<dyn SecurityOperations>> {
    grants
        .into_iter()
        .filter_map(|grant| match grant {
            Grant::Mount(mount) => {
                let policy = mount.policy()?;
                Some(Arc::new(MountPolicy {
                    mnt: mount.mnt().clone(),
                    at: mount.at().to_string(),
                    policy: policy.clone(),
                }) as Arc<dyn SecurityOperations>)
            }
            Grant::Network(network) => {
                let policy = network.policies().connect?;
                Some(Arc::new(EntryPolicy(policy)) as Arc<dyn SecurityOperations>)
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filesystem::wrap::{Hook, Wrap};
    use crate::filesystem::{DentryOperations, Filesystem, InodeOperations, Tmpfs};

    /// A Policy allowing everything, counting how often it is asked.
    #[derive(Default)]
    struct Counts(std::sync::atomic::AtomicUsize);

    impl Policy<MountRequest<'_>> for Counts {
        fn allows(&self, _request: &MountRequest<'_>) -> bool {
            self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            true
        }
    }

    /// A Policy refusing every change beneath `/guarded/q`.
    struct RefusesChangesInQ;

    impl Policy<MountRequest<'_>> for RefusesChangesInQ {
        fn allows(&self, request: &MountRequest<'_>) -> bool {
            request.access == Access::Read || !request.path.starts_with("/guarded/q/")
        }
    }

    // The Policy refuses a change where the file stands by the truncate, so only
    // a decision kept from the open lets it through, as Landlock keeps the
    // truncate right it allowed on the file.
    // @behavior RT-272
    #[test]
    fn a_truncate_follows_what_the_policy_allowed_as_the_file_opened() {
        let filesystem = Filesystem::new(Arc::new(
            [("p/f", "x"), ("q/keep", "x")]
                .into_iter()
                .collect::<Tmpfs>(),
        ))
        .expect("the instance must stand");
        let guarded = Mount::new("/guarded", filesystem.clone(), Access::ReadWrite)
            .expect("/guarded is a guest path")
            .with_policy(Arc::new(RefusesChangesInQ));
        let plain =
            Mount::new("/plain", filesystem, Access::ReadWrite).expect("/plain is a guest path");
        let cred = restrict_self(
            &prepare_creds_within(&[], None),
            [&Grant::Mount(guarded.clone())],
        );
        let write = OpenHow {
            write: true,
            ..Default::default()
        };
        let mut held =
            crate::vfs::kern_path::filp_open(&cred, &Path::root(guarded.mnt()), "p/f", &write)
                .unwrap_or_else(|_| panic!("the file must open"));
        let root = Path::root(plain.mnt());
        crate::vfs::filename_renameat2(
            &restrict_self(&prepare_creds_within(&[], None), []),
            &root,
            "p/f",
            &root,
            "q/f",
        )
        .expect("the move must land");
        assert_eq!(
            held.truncate(0),
            Ok(()),
            "File::truncate of a file opened to write beneath a Policy, moved since to where the \
             Policy refuses changes, must follow what the Policy allowed as it opened"
        );
    }

    // A Policy refusing nothing still counts each ask, so only an open asking
    // no module leaves it at zero.
    // @behavior RT-302
    #[test]
    fn an_open_standing_for_a_path_asks_no_policy() {
        let policy = Arc::new(Counts::default());
        let filesystem = Filesystem::new(Arc::new([("f", "x")].into_iter().collect::<Tmpfs>()))
            .expect("the instance must stand");
        let mount = Mount::new("/data", filesystem, Access::ReadWrite)
            .expect("/data is a guest path")
            .with_policy(policy.clone());
        let cred = restrict_self(
            &prepare_creds_within(&[], None),
            [&Grant::Mount(mount.clone())],
        );
        let opened = crate::vfs::kern_path::filp_open(
            &cred,
            &Path::root(mount.mnt()),
            "f",
            &OpenHow::default(),
        );
        assert_eq!(
            (
                opened.map(|file| file.is_path()),
                policy.0.load(std::sync::atomic::Ordering::SeqCst)
            ),
            (Ok(true), 0),
            "vfs::do_file_open of a file asking neither to read nor to write, beneath a Policy, \
             must stand for the file without asking the Policy, as Linux's do_o_path asks no \
             security module"
        );
    }

    /// What making an entry answers through a directory moved past
    /// `PATH_MAX`, beneath a mount `prepare` finishes.
    fn made_past_the_limit(prepare: impl FnOnce(Mount) -> Mount) -> Result<(), Errno> {
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
        let cred = restrict_self(
            &prepare_creds_within(&[], None),
            [&Grant::Mount(mount.clone())],
        );
        let root = Path::root(mount.mnt());
        let how = crate::vfs::OpenHow {
            read: true,
            directory: true,
            ..Default::default()
        };
        let held = crate::vfs::kern_path::filp_open(&cred, &root, &format!("c0/{deep}"), &how)
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
            (Err(Errno::InvalidFilename), 1),
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
        ) -> Result<bool, Errno> {
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
        let cred = restrict_self(
            &prepare_creds_within(&[], None),
            [&Grant::Mount(guarded.clone())],
        );
        let how = crate::vfs::OpenHow {
            read: true,
            directory: true,
            ..Default::default()
        };
        let held = crate::vfs::kern_path::filp_open(&cred, &Path::root(guarded.mnt()), "p/d", &how)
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
            crate::vfs::filename_renameat2(
                &restrict_self(&prepare_creds_within(&[], None), []),
                &root,
                "p/d",
                &root,
                "q/d",
            )
            .expect("the move must land");
            crate::sched::testing::join_scoped("the call", first)
        });
        assert!(
            first.is_ok()
                && guard.check(held.path()).is_ok()
                && crate::vfs::vfs_fstatat(&cred, held.path(), "f", false).is_ok()
                && held.path().dentry().path_raw() == "/q/d",
            "WalkGuard::check of a directory moved by another Mount while revalidate is asked, \
             beneath a Policy, must let a call through both then and afterwards, at /q/d"
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
        ) -> Result<bool, Errno> {
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
                        &restrict_self(&prepare_creds_within(&[], None), []),
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

    // Each pass from the root meets a move another Mount makes, so a guard asking
    // again would make the call wait while the moves go on; one pass answers,
    // and the caller makes the call again, as a scoped walk's `..` does.
    // @behavior RT-266
    #[test]
    fn a_start_moved_while_it_is_asked_answers_eagain_beneath_a_policy() {
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
            (Err(Errno::WouldBlock), 1),
            "WalkGuard::check of a directory another Mount moves while it is asked, beneath a \
             Policy, must answer EAGAIN from the pass that met the move"
        );
    }

    mod stress {
        use std::sync::atomic::{AtomicBool, Ordering};

        use super::*;

        // Nothing refuses a directory that stands, so a single refusal while it
        // moves between two names is a dentry the walk let go of. A move met
        // while the guard asks answers EAGAIN for the caller to ask again, which
        // refuses nothing.
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
                    let (root, cred) = (
                        Path::root(plain.mnt()),
                        restrict_self(&prepare_creds_within(&[], None), []),
                    );
                    while !stop.load(Ordering::SeqCst) {
                        for (from, to) in [("p/d", "q/d"), ("q/d", "p/d")] {
                            crate::vfs::filename_renameat2(&cred, &root, from, &root, to)
                                .expect("the move must land");
                        }
                    }
                });
                let call = || {
                    guard.check(&start)?;
                    crate::vfs::vfs_fstatat(&cred, &start, "f", false)
                };
                let refused = (0..200_000)
                    .filter(|_| call().is_err_and(|error| error != Errno::WouldBlock))
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
                name: &|| "/".to_owned(),
            };
            KobakoSecurity
                .file_open(path, kind, FMode::READ, &OpenHow::default())
                .is_ok()
        };
        assert_eq!(
            [
                FileType::RegFile,
                FileType::Dir,
                FileType::Symlink,
                FileType::Fifo,
                FileType::Sock,
                FileType::Blkdev,
                FileType::Chrdev,
                FileType::Unknown,
            ]
            .map(judged),
            [true, true, false, false, false, false, false, false],
            "KobakoSecurity::file_open must pass a file and a directory, and refuse every other \
             kind"
        );
    }
}
