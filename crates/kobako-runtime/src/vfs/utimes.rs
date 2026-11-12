//! Setting the times of an entry, as Linux's `utimensat` does. What answers is
//! the entry's own `setattr`, so a Filesystem decides whether it keeps a time
//! of the caller's choosing.

use std::sync::Arc;

use super::attr::notify_change;
use super::namei::{filename_lookup, retry_estale};
use super::path::{Path, Start};
use super::{Errno, Iattr};
use crate::cred::Cred;
use crate::security::on_path;

/// Set `times` on what the descriptor at `path` names, as Linux's `do_utimes_fd`.
pub fn do_utimes_fd(cred: &Arc<Cred>, path: &Path, times: &Iattr) -> Result<(), Errno> {
    vfs_utimes(cred, path, times)
}

/// Set `times` on what `path` names, as Linux's `vfs_utimes` asks the mount,
/// then the security hook, then `setattr` under the entry's lock. An entry
/// without `setattr` keeps no time to set, as this VFS holds none on an inode.
fn vfs_utimes(cred: &Arc<Cred>, path: &Path, times: &Iattr) -> Result<(), Errno> {
    path.mnt().mnt_want_write()?;
    let dentry = path.dentry();
    on_path(path.mnt(), dentry, |at| cred.security_path_utimes(at))?;
    let held = dentry.inode_lock()?;
    notify_change(dentry, &held, times)
}

/// Set `times` on what `path` beneath `start` names, a link at its end
/// followed where `follow` asks, as Linux's `do_utimes_path` walks to it first.
pub fn do_utimes_path(
    cred: &Arc<Cred>,
    start: &impl Start,
    path: &str,
    follow: bool,
    times: &Iattr,
) -> Result<(), Errno> {
    retry_estale(
        || filename_lookup(cred, start, path, follow),
        |nd| vfs_utimes(cred, &nd.path_to(nd.path()?), times),
    )
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::time::UNIX_EPOCH;

    use super::*;
    use crate::filesystem::{
        FileSystemType, FileType, Filesystem, InodeOperations, Kstat, SetTime, Tmpfs,
    };
    use crate::grant::Policy;
    use crate::mount::{Access as MountAccess, Mount, MountRequest};
    use crate::security::{prepare_creds_within, restrict_self};
    use crate::vfs::Dentry;

    fn times() -> Iattr<'static> {
        Iattr::times(Some(SetTime::Now), Some(SetTime::At(UNIX_EPOCH)))
    }

    fn mount_of(filesystem: Arc<dyn FileSystemType>, access: MountAccess) -> Mount {
        let filesystem = Filesystem::new(filesystem).expect("the instance must stand");
        Mount::new("/data", filesystem, access).expect("/data is a guest path")
    }

    fn tmpfs() -> Arc<Tmpfs> {
        Arc::new([("file.txt", "x")].into_iter().collect())
    }

    // A thread waiting on a lock it holds answers a deadlock, so only a change
    // that takes the entry's lock answers one here.
    // @behavior RT-236
    #[test]
    fn a_change_to_an_entry_holds_its_lock() {
        let mount = mount_of(tmpfs(), MountAccess::ReadWrite);
        let root = Path::root(mount.mnt());
        let _held = root.dentry().inode_lock().expect("the lock must be free");
        assert_eq!(
            vfs_utimes(&Arc::default(), &root, &times()),
            Err(Errno::Deadlock),
            "vfs::vfs_utimes on an entry whose lock its own thread holds must answer EDEADLK"
        );
    }

    // @behavior RT-236
    #[test]
    fn a_truncate_through_a_descriptor_holds_the_entry_lock() {
        let mount = mount_of(tmpfs(), MountAccess::ReadWrite);
        let write = crate::vfs::OpenHow {
            write: true,
            ..Default::default()
        };
        let mut file = crate::vfs::kern_path::filp_open(
            &Arc::default(),
            &Path::root(mount.mnt()),
            "file.txt",
            &write,
        )
        .expect("the file opens to write");
        let dentry = file.path().dentry().clone();
        let _held = dentry.inode_lock().expect("the lock must be free");
        assert_eq!(
            file.truncate(0),
            Err(Errno::Deadlock),
            "vfs::File::truncate on a file whose lock its own thread holds must answer EDEADLK, \
             as do_ftruncate holds the inode's lock around notify_change"
        );
    }

    // @behavior RT-186
    #[test]
    fn setting_times_asks_the_mount_then_the_entry() {
        let set = |access, path: &str| {
            let mount = mount_of(tmpfs(), access);
            do_utimes_path(
                &Arc::default(),
                &Path::root(mount.mnt()),
                path,
                false,
                &times(),
            )
        };
        assert_eq!(
            (
                set(MountAccess::ReadWrite, "missing"),
                set(MountAccess::Read, "file.txt"),
                set(MountAccess::ReadWrite, "file.txt"),
            ),
            (
                Err(Errno::NotFound),
                Err(Errno::ReadOnlyFilesystem),
                Ok(()),
            ),
            "vfs::do_utimes_path must answer what is not there ENOENT, a read-only Mount EROFS, and \
             hand a Tmpfs entry the times"
        );
    }

    /// A directory answering every call but `setattr` by default.
    struct NoSetattr;

    impl FileSystemType for NoSetattr {
        fn fill_super(self: Arc<Self>, sb: &mut Filesystem) -> Result<(), Errno> {
            sb.d_make_root(crate::vfs::Inode::new(
                &crate::vfs::Kstat::new(crate::vfs::FileType::Dir, 0),
                self,
            ));
            Ok(())
        }
    }

    impl InodeOperations for NoSetattr {
        fn getattr(&self, _dentry: &Dentry) -> Result<Kstat, Errno> {
            Ok(Kstat::new(FileType::Dir, 0))
        }
    }

    // @behavior RT-186
    #[test]
    fn an_entry_without_setattr_answers_enotsup() {
        let mount = mount_of(Arc::new(NoSetattr), MountAccess::ReadWrite);
        assert_eq!(
            vfs_utimes(&Arc::default(), &Path::root(mount.mnt()), &times()),
            Err(Errno::Unsupported),
            "vfs::vfs_utimes on an entry without setattr must answer ENOTSUP"
        );
    }

    /// A Filesystem keeping what it was last asked to set.
    #[derive(Default)]
    struct KeepsTimes(Mutex<Option<Iattr<'static>>>);

    impl FileSystemType for KeepsTimes {
        fn fill_super(self: Arc<Self>, sb: &mut Filesystem) -> Result<(), Errno> {
            sb.d_make_root(crate::vfs::Inode::new(
                &crate::vfs::Kstat::new(crate::vfs::FileType::Dir, 0),
                self,
            ));
            Ok(())
        }
    }

    impl InodeOperations for KeepsTimes {
        fn getattr(&self, _dentry: &Dentry) -> Result<Kstat, Errno> {
            Ok(Kstat::new(FileType::Dir, 0))
        }

        fn setattr(&self, _dentry: &Dentry, attr: &Iattr) -> Result<(), Errno> {
            *self.0.lock().unwrap() = Some(Iattr {
                size: attr.size,
                atime: attr.atime,
                mtime: attr.mtime,
                open: attr.open,
                ctime: attr.ctime,
                file: None,
            });
            Ok(())
        }
    }

    // @behavior RT-186
    #[test]
    fn a_filesystem_with_setattr_answers_for_the_times_itself() {
        let filesystem = Arc::new(KeepsTimes::default());
        let mount = mount_of(filesystem.clone(), MountAccess::ReadWrite);
        let answer = vfs_utimes(&Arc::default(), &Path::root(mount.mnt()), &times());
        let handed = filesystem
            .0
            .lock()
            .unwrap()
            .map(|attr| (attr.atime, attr.mtime, attr.ctime.is_some()));
        assert_eq!(
            (answer, handed),
            (Ok(()), Some((times().atime, times().mtime, true))),
            "vfs::vfs_utimes on a Filesystem with setattr must hand it the times and the moment \
             the change lands, and answer what it answers"
        );
    }

    /// A Policy allowing everything, keeping each path it was asked about.
    #[derive(Default)]
    struct RecordsPaths(Mutex<Vec<String>>);

    impl Policy<MountRequest<'_>> for RecordsPaths {
        fn allows(&self, request: &MountRequest<'_>) -> bool {
            self.0.lock().unwrap().push(request.path.to_string());
            true
        }
    }

    // A file's descriptor outlives its name, so only the walk's own check
    // keeps a Policy from judging a path it no longer stands at.
    // @behavior RT-201
    #[test]
    fn a_time_set_through_a_descriptor_is_judged_where_it_stands() {
        let policy = Arc::new(RecordsPaths::default());
        let mount = mount_of(tmpfs(), MountAccess::ReadWrite).with_policy(policy.clone());
        let cred = restrict_self(
            &prepare_creds_within(&[], None),
            [&crate::grant::Grant::Mount(mount.clone())],
        );
        let root = Path::root(mount.mnt());
        let how = crate::vfs::OpenHow {
            read: true,
            ..Default::default()
        };
        let file = crate::vfs::kern_path::filp_open(&cred, &root, "file.txt", &how)
            .unwrap_or_else(|_| panic!("the file must open"));
        crate::vfs::filename_unlinkat(&cred, &root, "file.txt").expect("the name must go");
        policy.0.lock().unwrap().clear();
        assert_eq!(
            (
                crate::security::WalkGuard::new([&mount]).check(file.path()),
                policy.0.lock().unwrap().clone()
            ),
            (Err(Errno::NotFound), Vec::new()),
            "WalkGuard::check of a descriptor of a file whose name is gone, beneath a Policy, \
             must answer ENOENT without asking the Policy about the path it stood at"
        );
    }

    struct RefusesAll;

    impl Policy<MountRequest<'_>> for RefusesAll {
        fn allows(&self, _request: &MountRequest<'_>) -> bool {
            false
        }
    }

    // A file opened to read reaches this with no write the Policy judged, so
    // the call itself must ask.
    // @behavior RT-186
    #[test]
    fn a_policy_is_asked_before_times_are_set() {
        let filesystem = Arc::new(KeepsTimes::default());
        let mount =
            mount_of(filesystem.clone(), MountAccess::ReadWrite).with_policy(Arc::new(RefusesAll));
        assert_eq!(
            (
                vfs_utimes(
                    &restrict_self(
                        &prepare_creds_within(&[], None),
                        [&crate::grant::Grant::Mount(mount.clone())],
                    ),
                    &Path::root(mount.mnt()),
                    &times()
                ),
                *filesystem.0.lock().unwrap(),
            ),
            (Err(Errno::PermissionDenied), None),
            "vfs::vfs_utimes beneath a Policy refusing the path must answer EACCES and leave the \
             Filesystem unasked"
        );
    }
}
