//! A sequence's calls through kobako's VFS.

use std::collections::BTreeMap;
use std::fmt::Debug;
use std::sync::Arc;
use std::time::SystemTime;

use kobako_runtime::filesystem::{Filesystem, FilesystemError};
use kobako_runtime::mount::{Access, Mount};
use kobako_runtime::security::prepare_creds;
use kobako_runtime::vfs::{self, Cred, DirContext, DirEmit, File, FileType, OpenHow, Whence};

use crate::calls::{payload, Call, SLOTS};
use crate::{describe, Snapshot, Times};

fn errno(error: &FilesystemError) -> String {
    error
        .posix_name()
        .map_or_else(|| format!("EIO({error:?})"), str::to_string)
}

fn answer<T>(result: Result<T, FilesystemError>, ok: impl FnOnce(T) -> String) -> String {
    result.map_or_else(|error| errno(&error), ok)
}

fn done<T: Debug>(result: Result<T, FilesystemError>) -> String {
    answer(result, |_| "ok".to_string())
}

fn kind_name(kind: FileType) -> &'static str {
    match kind {
        FileType::File => "reg",
        FileType::Directory => "dir",
        FileType::Symlink => "lnk",
        _ => "oth",
    }
}

fn directory_how() -> OpenHow {
    let mut how = OpenHow::default();
    how.read = true;
    how.directory = true;
    how
}

/// The names `file` lists, kinds included, `.` and `..` among them.
fn entries(file: &mut File) -> Result<Vec<(String, FileType)>, FilesystemError> {
    let mut names = Vec::new();
    let mut actor = |emit: &DirEmit<'_>| {
        names.push((emit.name.to_string(), emit.file_type));
        true
    };
    file.iterate_dir(&mut DirContext::new(0, &mut actor))?;
    Ok(names)
}

pub struct Kobako {
    cred: Arc<Cred>,
    root: File,
    slots: Vec<Option<File>>,
    merged: bool,
}

impl Kobako {
    /// The root of `filesystem` mounted with `access`. `merged` leaves out what
    /// an Overlay and overlayfs each count of a directory their own way.
    pub fn new(filesystem: Arc<Filesystem>, access: Access, merged: bool) -> Self {
        let mount = Mount::new("/baseline", filesystem, access).expect("/baseline is a guest path");
        let cred = prepare_creds([&mount]);
        let root =
            File::root(mount.mnt()).unwrap_or_else(|error| panic!("the root must open: {error:?}"));
        Self {
            cred,
            root,
            slots: std::iter::repeat_with(|| None).take(SLOTS).collect(),
            merged,
        }
    }

    fn start(&self, base: Option<usize>) -> Result<&vfs::Path, String> {
        match base {
            None => Ok(self.root.path()),
            Some(slot) => match &self.slots[slot] {
                None => Err("EBADF".to_string()),
                Some(file) => Ok(file.path()),
            },
        }
    }

    fn stat(&self, metadata: vfs::Metadata) -> String {
        describe(
            kind_name(metadata.file_type),
            metadata.len,
            metadata.nlink,
            self.merged,
        )
    }

    pub fn run(&mut self, call: &Call) -> String {
        macro_rules! start {
            ($base:expr) => {
                match self.start($base) {
                    Ok(path) => path,
                    Err(answer) => return answer,
                }
            };
        }
        macro_rules! file {
            ($slot:expr) => {
                match self.slots[$slot].as_mut() {
                    Some(file) => file,
                    None => return "EBADF".to_string(),
                }
            };
        }
        match call {
            Call::Open(open) => {
                let mut how = OpenHow::default();
                how.read = open.read || !open.write;
                how.write = open.write;
                how.create = open.create;
                how.exclusive = open.exclusive;
                how.truncate = open.truncate;
                how.append = open.append;
                how.directory = open.directory;
                how.follow = !open.nofollow;
                let opened = vfs::do_file_open(&self.cred, start!(open.base), &open.path, &how);
                answer(opened, |file| {
                    self.slots[open.slot] = Some(file);
                    "ok".to_string()
                })
            }
            Call::Close(slot) => {
                file!(*slot);
                let file = self.slots[*slot].take().expect("the slot holds a file");
                done(file.close())
            }
            Call::Read(slot, len) => {
                let mut buf = vec![0; *len];
                answer(file!(*slot).read(&mut buf), |read| bytes(&buf[..read]))
            }
            Call::Write(slot, len) => answer(file!(*slot).write(payload(*len)), wrote),
            Call::Pread(slot, len, pos) => {
                let mut buf = vec![0; *len];
                answer(file!(*slot).pread(&mut buf, *pos), |read| {
                    bytes(&buf[..read])
                })
            }
            Call::Pwrite(slot, len, pos) => answer(file!(*slot).pwrite(payload(*len), *pos), wrote),
            Call::Seek(slot, offset, whence) => {
                let file = file!(*slot);
                if file.is_directory() {
                    return "directory".to_string();
                }
                let whence = [Whence::Set, Whence::Cur, Whence::End][*whence as usize];
                answer(file.llseek(*offset, whence), |pos| format!("ok:{pos}"))
            }
            Call::Truncate(slot, len) => done(file!(*slot).truncate(*len)),
            Call::Fstat(slot) => {
                let got = file!(*slot).getattr();
                answer(got, |metadata| self.stat(metadata))
            }
            Call::Fsync(slot) => done(file!(*slot).fsync()),
            Call::Readdir(slot) => answer(entries(file!(*slot)), |names| {
                let mut names: Vec<_> = names
                    .into_iter()
                    .map(|(name, kind)| format!("{name}:{}", kind_name(kind)))
                    .collect();
                names.sort();
                format!("ok:{names:?}")
            }),
            Call::Stat { base, path, follow } => {
                let got = vfs::vfs_fstatat(&self.cred, start!(*base), path, *follow);
                answer(got, |metadata| self.stat(metadata))
            }
            Call::Mkdir(base, path) => done(vfs::filename_mkdirat(&self.cred, start!(*base), path)),
            Call::Rmdir(base, path) => done(vfs::filename_rmdir(&self.cred, start!(*base), path)),
            Call::Unlink(base, path) => {
                done(vfs::filename_unlinkat(&self.cred, start!(*base), path))
            }
            Call::Rename(base, from, to) => done(vfs::filename_renameat2(
                &self.cred,
                start!(*base),
                from,
                self.root.path(),
                to,
            )),
        }
    }

    /// Visit every entry beneath the root, directories before what they hold.
    fn walk(&self, dir: &vfs::Path, prefix: &str, visit: &mut dyn FnMut(&vfs::Path, &str, &str)) {
        let Ok(mut opened) = vfs::do_file_open(&self.cred, dir, ".", &directory_how()) else {
            return;
        };
        for (name, kind) in entries(&mut opened).unwrap_or_default() {
            if name == "." || name == ".." {
                continue;
            }
            let full = format!("{prefix}{name}");
            visit(dir, &name, &full);
            if kind == FileType::Directory {
                if let Ok(sub) = vfs::do_file_open(&self.cred, dir, &name, &directory_how()) {
                    self.walk(sub.path(), &format!("{full}/"), visit);
                }
            }
        }
    }

    /// Every entry beneath the root, with a file's content.
    pub fn snapshot(&self) -> Snapshot {
        let mut out = BTreeMap::new();
        self.walk(self.root.path(), "", &mut |dir, name, full| {
            let seen = match vfs::vfs_fstatat(&self.cred, dir, name, false) {
                Err(error) => errno(&error),
                Ok(metadata) => {
                    let kind = metadata.file_type;
                    let mut seen = self.stat(metadata);
                    if kind == FileType::File {
                        seen += &self.content(dir, name);
                    }
                    seen
                }
            };
            out.insert(full.to_string(), seen);
        });
        out
    }

    fn content(&self, dir: &vfs::Path, name: &str) -> String {
        let mut how = OpenHow::default();
        how.read = true;
        let Ok(mut file) = vfs::do_file_open(&self.cred, dir, name, &how) else {
            return " unreadable".to_string();
        };
        let mut buf = [0; 64];
        let read = file.read(&mut buf).unwrap_or(0);
        format!(" {:?}", String::from_utf8_lossy(&buf[..read]))
    }

    /// When each entry beneath the root was last modified and changed.
    pub fn times(&self) -> Times<Option<SystemTime>> {
        let mut out = BTreeMap::new();
        if let Ok(metadata) = vfs::vfs_fstatat(&self.cred, self.root.path(), ".", false) {
            out.insert(".".to_string(), (metadata.modified, metadata.changed));
        }
        self.walk(self.root.path(), "", &mut |dir, name, full| {
            if let Ok(metadata) = vfs::vfs_fstatat(&self.cred, dir, name, false) {
                out.insert(full.to_string(), (metadata.modified, metadata.changed));
            }
        });
        out
    }
}

fn bytes(read: &[u8]) -> String {
    format!("ok:{:?}", String::from_utf8_lossy(read))
}

fn wrote(count: usize) -> String {
    format!("ok:{count}")
}
