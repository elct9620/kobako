//! A sequence's calls through the Linux the test runs on, syscall by syscall.

use std::collections::BTreeMap;
use std::ffi::{CStr, CString};
use std::fs;
use std::io;
use std::mem;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

use crate::calls::{payload, Call, SLOTS};
use crate::{describe, Snapshot, Times};

fn c_string(text: &str) -> CString {
    CString::new(text).expect("no path of a sequence holds a NUL")
}

/// The name of the errno the last syscall left, as kobako names a failure.
fn errno() -> String {
    let code = io::Error::last_os_error().raw_os_error().unwrap_or(0);
    let name = match code {
        libc::ENOENT => "ENOENT",
        libc::EEXIST => "EEXIST",
        libc::ENOTDIR => "ENOTDIR",
        libc::EISDIR => "EISDIR",
        libc::ENOTEMPTY => "ENOTEMPTY",
        libc::EACCES => "EACCES",
        libc::EROFS => "EROFS",
        libc::EINVAL => "EINVAL",
        libc::ENOTSUP => "ENOTSUP",
        libc::ENOSPC => "ENOSPC",
        libc::EFBIG => "EFBIG",
        libc::EXDEV => "EXDEV",
        libc::ELOOP => "ELOOP",
        libc::EPERM => "EPERM",
        libc::EBADF => "EBADF",
        libc::EBUSY => "EBUSY",
        libc::ESPIPE => "ESPIPE",
        libc::ENAMETOOLONG => "ENAMETOOLONG",
        other => return format!("errno {other}"),
    };
    name.to_string()
}

fn answer(result: i64, ok: impl FnOnce(i64) -> String) -> String {
    match result {
        failed if failed < 0 => errno(),
        done => ok(done),
    }
}

fn ok_or_errno(result: i64) -> String {
    answer(result, |_| "ok".to_string())
}

fn bytes(buf: &[u8], read: i64) -> String {
    format!("ok:{:?}", String::from_utf8_lossy(&buf[..read as usize]))
}

fn bytes_written(count: i64) -> String {
    format!("ok:{count}")
}

/// A `st_nlink`, whose width differs between architectures.
fn nlink(count: impl Into<u64>) -> u64 {
    count.into()
}

fn kind_of(mode: u32) -> &'static str {
    match mode & libc::S_IFMT {
        libc::S_IFREG => "reg",
        libc::S_IFDIR => "dir",
        libc::S_IFLNK => "lnk",
        _ => "oth",
    }
}

/// Mount `source` at `target`, naming what stopped it, since nothing here is
/// worth holding if the filesystem the sequence runs on is not the real one.
pub fn mount(source: &str, target: &str, kind: Option<&str>, flags: u64, data: Option<&str>) {
    let (source, target) = (c_string(source), c_string(target));
    let kind = kind.map(c_string);
    let data = data.map(c_string);
    // SAFETY: each pointer is a live NUL-terminated string or null.
    let result = unsafe {
        libc::mount(
            source.as_ptr(),
            target.as_ptr(),
            kind.as_ref().map_or(std::ptr::null(), |kind| kind.as_ptr()),
            flags,
            data.as_ref()
                .map_or(std::ptr::null(), |data| data.as_ptr().cast()),
        )
    };
    assert_eq!(
        result,
        0,
        "mounting {target:?} answered {}; this test needs root and a mount namespace, which \
         `rake crates:linux_baseline` runs it with",
        errno()
    );
}

/// Let go of what is mounted at `target`, if anything is.
pub fn umount(target: &str) {
    let target = c_string(target);
    // SAFETY: `target` is a live NUL-terminated string.
    unsafe { libc::umount(target.as_ptr()) };
}

/// Whether `path` stands on a tmpfs, whose answers this test holds a Tmpfs to.
pub fn on_tmpfs(path: &str) -> bool {
    let path = c_string(path);
    // SAFETY: `stat` is written before it is read, and only on success.
    unsafe {
        let mut stat: libc::statfs = mem::zeroed();
        libc::statfs(path.as_ptr(), &mut stat) == 0 && stat.f_type == libc::TMPFS_MAGIC
    }
}

pub struct Linux {
    root: i32,
    root_path: String,
    slots: [Option<i32>; SLOTS],
    merged: bool,
}

impl Linux {
    /// The directory at `root_path`. `merged` leaves out what an Overlay and
    /// overlayfs each count of a directory their own way.
    pub fn new(root_path: &str, merged: bool) -> Self {
        let path = c_string(root_path);
        // SAFETY: `path` is a live NUL-terminated string.
        let root = unsafe { libc::open(path.as_ptr(), libc::O_DIRECTORY | libc::O_RDONLY) };
        assert!(root >= 0, "{root_path} must open: {}", errno());
        Self {
            root,
            root_path: root_path.to_string(),
            slots: [None; SLOTS],
            merged,
        }
    }

    fn start(&self, base: Option<usize>) -> Result<i32, String> {
        match base {
            None => Ok(self.root),
            Some(slot) => self.slots[slot].ok_or_else(|| "EBADF".to_string()),
        }
    }

    fn fstat(fd: i32) -> Option<libc::stat> {
        // SAFETY: `stat` is written before it is read, and only on success.
        unsafe {
            let mut stat: libc::stat = mem::zeroed();
            (libc::fstat(fd, &mut stat) == 0).then_some(stat)
        }
    }

    fn is_directory(fd: i32) -> bool {
        Self::fstat(fd).is_some_and(|stat| stat.st_mode & libc::S_IFMT == libc::S_IFDIR)
    }

    fn stat(&self, stat: &libc::stat) -> String {
        describe(
            kind_of(stat.st_mode),
            stat.st_size as u64,
            nlink(stat.st_nlink),
            self.merged,
        )
    }

    /// What `fd` lists from its start, kinds included, sorted.
    fn readdir(fd: i32) -> String {
        const DT_NAME_AT: usize = 19;
        // SAFETY: `getdents64` writes at most `buf.len()` bytes, and each
        // record it answers holds a NUL-terminated name within what it wrote.
        unsafe {
            if Self::is_directory(fd) {
                libc::lseek(fd, 0, libc::SEEK_SET);
            }
            let mut names = Vec::new();
            let mut buf = vec![0u8; 8192];
            loop {
                let read = libc::syscall(libc::SYS_getdents64, fd, buf.as_mut_ptr(), buf.len());
                match read {
                    failed if failed < 0 => return errno(),
                    0 => break,
                    _ => {}
                }
                let mut at = 0;
                while at < read as usize {
                    let length = u16::from_ne_bytes([buf[at + 16], buf[at + 17]]) as usize;
                    let kind = match buf[at + 18] {
                        libc::DT_REG => "reg",
                        libc::DT_DIR => "dir",
                        libc::DT_LNK => "lnk",
                        _ => "oth",
                    };
                    let name = CStr::from_ptr(buf.as_ptr().add(at + DT_NAME_AT).cast());
                    names.push(format!("{}:{kind}", name.to_string_lossy()));
                    at += length;
                }
            }
            names.sort();
            format!("ok:{names:?}")
        }
    }

    fn open(&mut self, open: &crate::calls::Open) -> String {
        let at = match self.start(open.base) {
            Ok(at) => at,
            Err(answer) => return answer,
        };
        let mut flags = libc::O_CLOEXEC
            | match (open.read || !open.write, open.write) {
                (true, true) => libc::O_RDWR,
                (false, true) => libc::O_WRONLY,
                _ => libc::O_RDONLY,
            };
        for (asked, flag) in [
            (open.create, libc::O_CREAT),
            (open.exclusive, libc::O_EXCL),
            (open.truncate, libc::O_TRUNC),
            (open.append, libc::O_APPEND),
            (open.directory, libc::O_DIRECTORY),
            (open.nofollow, libc::O_NOFOLLOW),
        ] {
            if asked {
                flags |= flag;
            }
        }
        let path = c_string(&open.path);
        // SAFETY: `path` is a live NUL-terminated string.
        let fd = unsafe { libc::openat(at, path.as_ptr(), flags, 0o644) };
        if fd < 0 {
            return errno();
        }
        if let Some(old) = self.slots[open.slot].replace(fd) {
            // SAFETY: the slot held the only reference to `old`.
            unsafe { libc::close(old) };
        }
        "ok".to_string()
    }

    pub fn run(&mut self, call: &Call) -> String {
        macro_rules! start {
            ($base:expr) => {
                match self.start($base) {
                    Ok(at) => at,
                    Err(answer) => return answer,
                }
            };
        }
        macro_rules! fd {
            ($slot:expr) => {
                match self.slots[$slot] {
                    Some(fd) => fd,
                    None => return "EBADF".to_string(),
                }
            };
        }
        // SAFETY: each path is a live NUL-terminated string, each buffer outlives
        // the syscall it is handed to, with its length, and each `fd` is open.
        unsafe {
            match call {
                Call::Open(open) => self.open(open),
                Call::Close(slot) => {
                    let fd = fd!(*slot);
                    self.slots[*slot] = None;
                    ok_or_errno(libc::close(fd).into())
                }
                Call::Read(slot, len) => {
                    let mut buf = vec![0u8; *len];
                    let read = libc::read(fd!(*slot), buf.as_mut_ptr().cast(), *len);
                    answer(read as i64, |read| bytes(&buf, read))
                }
                Call::Write(slot, len) => {
                    let data = payload(*len);
                    let count = libc::write(fd!(*slot), data.as_ptr().cast(), data.len());
                    answer(count as i64, bytes_written)
                }
                Call::Pread(slot, len, pos) => {
                    let mut buf = vec![0u8; *len];
                    let read = libc::pread(fd!(*slot), buf.as_mut_ptr().cast(), *len, *pos as i64);
                    answer(read as i64, |read| bytes(&buf, read))
                }
                Call::Pwrite(slot, len, pos) => {
                    let data = payload(*len);
                    let count =
                        libc::pwrite(fd!(*slot), data.as_ptr().cast(), data.len(), *pos as i64);
                    answer(count as i64, bytes_written)
                }
                Call::Seek(slot, offset, whence) => {
                    let fd = fd!(*slot);
                    if Self::is_directory(fd) {
                        return "directory".to_string();
                    }
                    let whence = [libc::SEEK_SET, libc::SEEK_CUR, libc::SEEK_END][*whence as usize];
                    answer(libc::lseek(fd, *offset, whence), |pos| format!("ok:{pos}"))
                }
                Call::Truncate(slot, len) => {
                    ok_or_errno(libc::ftruncate(fd!(*slot), *len as i64).into())
                }
                Call::Fstat(slot) => match Self::fstat(fd!(*slot)) {
                    Some(stat) => self.stat(&stat),
                    None => errno(),
                },
                Call::Fsync(slot) => ok_or_errno(libc::fsync(fd!(*slot)).into()),
                Call::Readdir(slot) => Self::readdir(fd!(*slot)),
                Call::Stat { base, path, follow } => {
                    let mut stat: libc::stat = mem::zeroed();
                    let flags = if *follow {
                        0
                    } else {
                        libc::AT_SYMLINK_NOFOLLOW
                    };
                    let path = c_string(path);
                    match libc::fstatat(start!(*base), path.as_ptr(), &mut stat, flags) {
                        0 => self.stat(&stat),
                        _ => errno(),
                    }
                }
                Call::Mkdir(base, path) => {
                    ok_or_errno(libc::mkdirat(start!(*base), c_string(path).as_ptr(), 0o755).into())
                }
                Call::Rmdir(base, path) => ok_or_errno(
                    libc::unlinkat(start!(*base), c_string(path).as_ptr(), libc::AT_REMOVEDIR)
                        .into(),
                ),
                Call::Unlink(base, path) => {
                    ok_or_errno(libc::unlinkat(start!(*base), c_string(path).as_ptr(), 0).into())
                }
                Call::Rename(base, from, to) => ok_or_errno(
                    libc::renameat(
                        start!(*base),
                        c_string(from).as_ptr(),
                        self.root,
                        c_string(to).as_ptr(),
                    )
                    .into(),
                ),
            }
        }
    }

    /// Every entry beneath the root, with a file's content.
    pub fn snapshot(&self) -> Snapshot {
        let mut out = BTreeMap::new();
        snapshot_into(Path::new(&self.root_path), "", self.merged, &mut out);
        out
    }

    /// When each entry beneath the root was last modified and changed.
    pub fn times(&self) -> Times<(i64, i64)> {
        fn walk(dir: &Path, prefix: &str, out: &mut Times<(i64, i64)>) {
            let Ok(entries) = fs::read_dir(dir) else {
                return;
            };
            for entry in entries.flatten() {
                let full = format!("{prefix}{}", entry.file_name().to_string_lossy());
                let metadata = fs::symlink_metadata(entry.path()).expect("the entry just listed");
                out.insert(
                    full.clone(),
                    (
                        (metadata.mtime(), metadata.mtime_nsec()),
                        (metadata.ctime(), metadata.ctime_nsec()),
                    ),
                );
                if metadata.is_dir() {
                    walk(&entry.path(), &format!("{full}/"), out);
                }
            }
        }
        let root = Path::new(&self.root_path);
        let metadata = fs::metadata(root).expect("the root stands");
        let mut out = BTreeMap::from([(
            ".".to_string(),
            (
                (metadata.mtime(), metadata.mtime_nsec()),
                (metadata.ctime(), metadata.ctime_nsec()),
            ),
        )]);
        walk(root, "", &mut out);
        out
    }
}

impl Drop for Linux {
    fn drop(&mut self) {
        for fd in self.slots.iter().flatten().chain([&self.root]) {
            // SAFETY: this holds the only reference to each descriptor.
            unsafe { libc::close(*fd) };
        }
    }
}

/// Every entry beneath `dir`, as a Filesystem's snapshot reads one.
pub fn snapshot_into(dir: &Path, prefix: &str, merged: bool, out: &mut Snapshot) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let full = format!("{prefix}{}", entry.file_name().to_string_lossy());
        let metadata = fs::symlink_metadata(entry.path()).expect("the entry just listed");
        let mut seen = describe(
            kind_of(metadata.mode()),
            metadata.len(),
            metadata.nlink(),
            merged,
        );
        if metadata.is_dir() {
            snapshot_into(&entry.path(), &format!("{full}/"), merged, out);
        }
        if metadata.is_file() {
            let content = fs::read(entry.path()).unwrap_or_default();
            let shown = &content[..content.len().min(64)];
            seen += &format!(" {:?}", String::from_utf8_lossy(shown));
        }
        out.insert(full, seen);
    }
}
