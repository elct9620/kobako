//! Setting the times of an entry, as Linux's `utimensat` does. What answers is
//! the entry's own `setattr`, so a Filesystem decides whether it keeps a time
//! of the guest's choosing.

use std::sync::Arc;

use super::cred::Cred;
use super::mount::Access;
use super::namei::filename_lookup;
use super::path::Path;
use super::{Error, FilesystemError, Iattr};

/// Set `times` on what `path` names, as Linux's `vfs_utimes` asks the mount,
/// then the security hook, then `setattr` under the entry's lock. An entry
/// without `setattr` keeps no time to set, as this VFS holds none on an inode.
pub fn vfs_utimes(cred: &Arc<Cred>, path: &Path, times: &Iattr) -> Result<(), Error> {
    path.mnt().mnt_want_write()?;
    cred.security_path_walk(path)?;
    let dentry = path.dentry();
    cred.security_inode_permission(path.mnt(), &|| dentry.path_raw(), Access::ReadWrite)?;
    let inode = dentry.inode().ok_or(FilesystemError::NotFound)?;
    let _held = dentry.inode_lock()?;
    Ok(inode
        .ops()
        .setattr(dentry, times)
        .unwrap_or(Err(FilesystemError::Unsupported))?)
}

/// Set `times` on what `path` beneath `start` names, a link at its end
/// followed where `follow` asks, as Linux's `do_utimes_path` walks to it first.
pub fn do_utimes(
    cred: &Arc<Cred>,
    start: &Path,
    path: &str,
    follow: bool,
    times: &Iattr,
) -> Result<(), Error> {
    let mut nd = filename_lookup(cred, start, path, follow, false)?;
    nd.check_directory()?;
    let dentry = match nd.name.is_empty() {
        true => nd.dentry().clone(),
        false => {
            nd.kind()?;
            let parent = nd.dentry().clone();
            let _held = parent.inode_lock_shared()?;
            nd.last_dentry()?
        }
    };
    vfs_utimes(cred, &start.with_dentry(dentry), times)
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::time::UNIX_EPOCH;

    use super::*;
    use crate::filesystem::{FileType, Filesystem, InodeOperations, Metadata, SetTime, Tmpfs};
    use crate::mount::{Access as MountAccess, Mount, Policy};
    use crate::security::prepare_creds;
    use crate::vfs::{Denial, Dentry};

    fn times() -> Iattr {
        Iattr::times(Some(SetTime::Now), Some(SetTime::At(UNIX_EPOCH)))
    }

    fn mount_of(filesystem: Arc<dyn InodeOperations>, access: MountAccess) -> Mount {
        Mount::new("/data", Filesystem::new(filesystem), access).expect("/data is a guest path")
    }

    fn tmpfs() -> Arc<Tmpfs> {
        Arc::new([("file.txt", "x")].into_iter().collect())
    }

    // @behavior RT-186
    #[test]
    fn setting_times_asks_the_mount_then_the_entry() {
        let set = |access, path: &str| {
            let mount = mount_of(tmpfs(), access);
            do_utimes(
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
                Err(Error::Filesystem(FilesystemError::NotFound)),
                Err(Error::Denied(Denial::ReadOnly)),
                Err(Error::Filesystem(FilesystemError::Unsupported)),
            ),
            "vfs::do_utimes must answer what is not there ENOENT, a read-only Mount EROFS, and \
             a Tmpfs entry, which keeps no time of the guest's choosing, ENOTSUP"
        );
    }

    /// A Filesystem keeping what it was last asked to set.
    #[derive(Default)]
    struct KeepsTimes(Mutex<Option<Iattr>>);

    impl InodeOperations for KeepsTimes {
        fn getattr(&self, _dentry: &Dentry) -> Result<Metadata, FilesystemError> {
            Ok(Metadata::new(FileType::Directory, 0))
        }

        fn setattr(&self, _dentry: &Dentry, attr: &Iattr) -> Option<Result<(), FilesystemError>> {
            *self.0.lock().unwrap() = Some(*attr);
            Some(Ok(()))
        }
    }

    // @behavior RT-186
    #[test]
    fn a_filesystem_with_setattr_answers_for_the_times_itself() {
        let filesystem = Arc::new(KeepsTimes::default());
        let mount = mount_of(filesystem.clone(), MountAccess::ReadWrite);
        assert_eq!(
            (
                vfs_utimes(&Arc::default(), &Path::root(mount.mnt()), &times()),
                *filesystem.0.lock().unwrap(),
            ),
            (Ok(()), Some(times())),
            "vfs::vfs_utimes on a Filesystem with setattr must hand it the times and answer what \
             it answers"
        );
    }

    /// A Policy allowing everything, keeping each path it was asked about.
    #[derive(Default)]
    struct RecordsPaths(Mutex<Vec<String>>);

    impl Policy for RecordsPaths {
        fn allows(&self, path: &str, _access: MountAccess) -> bool {
            self.0.lock().unwrap().push(path.to_string());
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
        let cred = prepare_creds([&mount]);
        let root = Path::root(mount.mnt());
        let how = crate::vfs::OpenHow {
            read: true,
            ..Default::default()
        };
        let file = crate::vfs::do_file_open(&cred, &root, "file.txt", &how)
            .unwrap_or_else(|_| panic!("the file must open"));
        crate::vfs::filename_unlinkat(&cred, &root, "file.txt").expect("the name must go");
        policy.0.lock().unwrap().clear();
        assert_eq!(
            (
                vfs_utimes(&cred, file.path(), &times()),
                policy.0.lock().unwrap().clone()
            ),
            (
                Err(Error::Filesystem(FilesystemError::NotFound)),
                Vec::new()
            ),
            "vfs::vfs_utimes through a descriptor of a file whose name is gone, beneath a Policy, \
             must answer ENOENT without asking the Policy about the path it stood at"
        );
    }

    struct RefusesAll;

    impl Policy for RefusesAll {
        fn allows(&self, _path: &str, _access: MountAccess) -> bool {
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
                vfs_utimes(&prepare_creds([&mount]), &Path::root(mount.mnt()), &times()),
                *filesystem.0.lock().unwrap(),
            ),
            (Err(Error::Denied(Denial::Refused)), None),
            "vfs::vfs_utimes beneath a Policy refusing the path must answer EACCES and leave the \
             Filesystem unasked"
        );
    }
}
