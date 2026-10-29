//! Integration coverage for `Sandbox::mount` and `Context::mount`: a
//! Filesystem the guest's `File` reaches beneath the mounted path, the
//! registration rules a mount shares with `bind`, and a policy deciding
//! each open.
#![cfg(all(feature = "msgpack", feature = "wasmtime"))]

use std::sync::Arc;

use crate::common::real_sandbox;
use kobako::filesystem::{
    DirEntry, File, FileType, FilesystemError, Inode, Metadata, OpenOptions, Tmpfs,
};
use kobako::msgpack::Value;
use kobako::{Access, Dentry, Error, Filesystem, Policy, Profile, Sandbox};
use kobako_runtime::dispatch::DispatchHandler;
use kobako_runtime::error::InvokeError;
use kobako_runtime::grant::Grant;
use kobako_runtime::runtime::{Entry, Runtime};
use kobako_runtime::snapshot::Snapshot;
use kobako_transport::envelope::Preamble;

const READ_NOTE: &str = r#"File.read("/data/note.txt")"#;
const RESCUE_DENIED: &str =
    r#"begin; File.read("/data/note.txt"); rescue Errno::EACCES; "denied"; end"#;

fn tmpfs(contents: &str) -> Arc<Filesystem> {
    Filesystem::new(Arc::new(
        [("note.txt", contents)].into_iter().collect::<Tmpfs>(),
    ))
}

fn sandbox_with_note() -> Option<Sandbox> {
    let mut sandbox = real_sandbox()?;
    sandbox
        .mount("/data", tmpfs("hello"), Access::Read)
        .expect("mount a Tmpfs at /data");
    Some(sandbox)
}

fn text(value: Value) -> String {
    match value {
        Value::Str(text) => text,
        other => panic!("expected a String, got {other:?}"),
    }
}

/// Refuses every path, standing in for a Host App's narrower rule.
struct Refuses;

impl Policy for Refuses {
    fn allows(&self, _path: &str, _access: Access) -> bool {
        false
    }
}

/// A runtime that presents no filesystem, so it never reaches a guest.
struct NoFilesystem;

impl Runtime for NoFilesystem {
    fn invoke(
        &self,
        _entry: Entry<'_>,
        _preamble: &Preamble,
        _handler: Option<Arc<dyn DispatchHandler>>,
        _grants: &[Grant],
    ) -> Result<Snapshot, InvokeError> {
        unreachable!("a mount is refused before any invocation")
    }

    fn profile(&self) -> Profile {
        Profile::Hermetic
    }
}

// @behavior MT-001
#[test]
fn a_mounted_filesystem_is_reachable_from_the_guest() {
    let Some(sandbox) = sandbox_with_note() else {
        return;
    };
    let value = sandbox.eval(READ_NOTE).unwrap().value().unwrap();

    assert_eq!(
        text(value),
        "hello",
        "a file in a Tmpfs mounted through Sandbox::mount must be read by the guest's File"
    );
}

// @behavior MT-002
#[test]
fn a_mount_after_the_first_invocation_is_refused() {
    let Some(mut sandbox) = sandbox_with_note() else {
        return;
    };
    sandbox.eval("nil").unwrap();

    assert!(
        matches!(
            sandbox.mount("/more", tmpfs(""), Access::Read),
            Err(Error::Sealed(_))
        ),
        "Sandbox::mount after the first invocation must be refused as sealed"
    );
}

// @behavior MT-003
#[test]
fn a_mount_path_that_is_not_absolute_is_refused() {
    let Some(mut sandbox) = real_sandbox() else {
        return;
    };
    for at in ["data", "/data/../etc", "/data/", "/a//b"] {
        assert!(
            matches!(
                sandbox.mount(at, tmpfs(""), Access::Read),
                Err(Error::Argument(_))
            ),
            "Sandbox::mount at {at:?} must be refused as no absolute guest path"
        );
    }
}

// @behavior MT-004
#[test]
fn a_path_mounted_twice_is_refused() {
    let Some(mut sandbox) = sandbox_with_note() else {
        return;
    };
    assert!(
        matches!(
            sandbox.mount("/data", tmpfs(""), Access::Read),
            Err(Error::Argument(_))
        ),
        "Sandbox::mount at a path already mounted must be refused"
    );
}

// The outer Tmpfs holds its own `inner/note.txt`, so only the innermost
// Mount answering keeps the read from reaching it.
// @behavior MT-005
#[test]
fn a_mount_nested_inside_another_reaches_its_own_filesystem() {
    let Some(mut sandbox) = real_sandbox() else {
        return;
    };
    let outer = Filesystem::new(Arc::new(
        [("inner/note.txt", "outer")].into_iter().collect::<Tmpfs>(),
    ));
    sandbox
        .mount("/data", outer, Access::Read)
        .expect("mount a Tmpfs at /data");
    sandbox
        .mount("/data/inner", tmpfs("inner"), Access::Read)
        .expect("mount a Tmpfs beneath /data");
    let value = sandbox
        .eval(r#"File.read("/data/inner/note.txt")"#)
        .unwrap()
        .value()
        .unwrap();

    assert_eq!(
        text(value),
        "inner",
        "a file read beneath a Sandbox::mount nested inside another must come from the inner \
         Filesystem"
    );
}

// @behavior MT-006
#[test]
fn a_runtime_presenting_no_filesystem_refuses_the_mount() {
    let mut sandbox = Sandbox::with_runtime(NoFilesystem, Profile::Hermetic).unwrap();

    assert!(
        matches!(
            sandbox.mount("/data", tmpfs(""), Access::Read),
            Err(Error::Setup(_))
        ),
        "Sandbox::mount over a runtime supporting no Mount must fail as Setup"
    );
}

// @behavior MT-007
#[test]
fn a_policy_refusing_an_open_denies_it_and_the_script_goes_on() {
    let Some(mut sandbox) = real_sandbox() else {
        return;
    };
    sandbox
        .mount_with_policy("/data", tmpfs("hello"), Arc::new(Refuses))
        .unwrap();
    let value = sandbox.eval(RESCUE_DENIED).unwrap().value().unwrap();

    assert_eq!(
        text(value),
        "denied",
        "an open a Sandbox::mount_with_policy policy refuses must reach the guest as Errno::EACCES"
    );
}

// @behavior MT-009
#[test]
fn a_context_replaces_a_mount_for_its_invocation_only() {
    let Some(sandbox) = sandbox_with_note() else {
        return;
    };
    let replaced = sandbox
        .eval_with(READ_NOTE, |ctx| {
            ctx.mount("/data", tmpfs("other"), Access::Read)
        })
        .unwrap()
        .value()
        .unwrap();
    let next = sandbox.eval(READ_NOTE).unwrap().value().unwrap();

    assert_eq!(
        (text(replaced), text(next)),
        ("other".to_string(), "hello".to_string()),
        "Context::mount must replace the mount for its own invocation and leave the next \
         reading the Sandbox's own mount"
    );
}

/// One read-only file, `note.txt`, written against `kobako::filesystem`
/// alone, as a Host App brings a Filesystem of its own.
struct OneNote(&'static [u8]);

struct NoteFile(&'static [u8]);

impl File for NoteFile {
    fn read_at(&mut self, buf: &mut [u8], offset: u64) -> Result<usize, FilesystemError> {
        let rest = self.0.get(offset as usize..).unwrap_or_default();
        let read = rest.len().min(buf.len());
        buf[..read].copy_from_slice(&rest[..read]);
        Ok(read)
    }

    fn write_at(&mut self, _buf: &[u8], _offset: u64) -> Result<usize, FilesystemError> {
        Err(FilesystemError::ReadOnlyFilesystem)
    }

    fn append(&mut self, _buf: &[u8]) -> Result<(u64, usize), FilesystemError> {
        Err(FilesystemError::ReadOnlyFilesystem)
    }

    fn metadata(&self) -> Result<Metadata, FilesystemError> {
        Ok(Metadata::new(FileType::File, self.0.len() as u64))
    }

    fn set_len(&mut self, _len: u64) -> Result<(), FilesystemError> {
        Err(FilesystemError::ReadOnlyFilesystem)
    }
}

impl Inode for OneNote {
    fn open(
        &self,
        dentry: &Dentry,
        _options: &OpenOptions,
    ) -> Result<Box<dyn File>, FilesystemError> {
        match dentry.name().as_str() {
            "note.txt" => Ok(Box::new(NoteFile(self.0))),
            _ => Err(FilesystemError::NotFound),
        }
    }

    fn metadata(&self, _dentry: &Dentry) -> Result<Metadata, FilesystemError> {
        Ok(Metadata::new(FileType::Directory, 0))
    }

    fn symlink_metadata(&self, dentry: &Dentry) -> Result<Metadata, FilesystemError> {
        match dentry.name().as_str() {
            "note.txt" => NoteFile(self.0).metadata(),
            _ => Err(FilesystemError::NotFound),
        }
    }

    fn open_dir(&self, dentry: &Dentry) -> Result<Arc<dyn Inode>, FilesystemError> {
        match dentry.name().as_str() {
            "note.txt" => Err(FilesystemError::NotADirectory),
            _ => Err(FilesystemError::NotFound),
        }
    }

    fn lookup(&self, dentry: &Dentry) -> Result<String, FilesystemError> {
        Ok(dentry.name())
    }

    fn d_revalidate(&self, _dentry: &Dentry) -> Result<bool, FilesystemError> {
        Ok(true)
    }

    fn read_link(&self, dentry: &Dentry) -> Result<String, FilesystemError> {
        self.symlink_metadata(dentry)?;
        Err(FilesystemError::InvalidInput)
    }

    fn entries(&self, _dentry: &Dentry) -> Result<Vec<DirEntry>, FilesystemError> {
        Ok(vec![DirEntry::new("note.txt", FileType::File)])
    }

    fn create_dir(&self, _dentry: &Dentry) -> Result<(), FilesystemError> {
        Err(FilesystemError::ReadOnlyFilesystem)
    }

    fn remove_dir(&self, _dentry: &Dentry) -> Result<(), FilesystemError> {
        Err(FilesystemError::ReadOnlyFilesystem)
    }

    fn remove_file(&self, _dentry: &Dentry) -> Result<(), FilesystemError> {
        Err(FilesystemError::ReadOnlyFilesystem)
    }

    fn rename(
        &self,
        _from: &Dentry,
        _to_dir: &dyn Inode,
        _to: &Dentry,
    ) -> Result<(), FilesystemError> {
        Err(FilesystemError::ReadOnlyFilesystem)
    }
}

// @behavior MT-021
#[test]
fn a_filesystem_a_rust_host_app_writes_mounts_like_the_kinds_kobako_ships() {
    let Some(mut sandbox) = real_sandbox() else {
        return;
    };
    sandbox
        .mount(
            "/data",
            Filesystem::new(Arc::new(OneNote(b"written in rust"))),
            Access::Read,
        )
        .unwrap();

    assert_eq!(
        text(sandbox.eval(READ_NOTE).unwrap().value().unwrap()),
        "written in rust",
        "a Filesystem a Host App implements against kobako::filesystem, mounted through \
         Sandbox::mount, must answer the guest's File"
    );
}

// @behavior MT-010
#[test]
fn a_context_cannot_mount_a_path_the_sandbox_never_mounted() {
    let Some(sandbox) = sandbox_with_note() else {
        return;
    };
    let result = sandbox.eval_with("nil", |ctx| ctx.mount("/other", tmpfs(""), Access::Read));

    assert!(
        matches!(result, Err(Error::Argument(_))),
        "Context::mount at a path the Sandbox never mounted must be refused before the guest runs"
    );
}
