//! Opening a directory several names beneath one held open, in one call
//! where the host's kernel confines a path itself: Linux's `openat2` with
//! `RESOLVE_BENEATH` and `RESOLVE_NO_SYMLINKS`, and from macOS 26
//! `O_RESOLVE_BENEATH` with `O_NOFOLLOW_ANY`. Elsewhere each name opens in
//! turn without following a link, which answers the same.

use cap_std::fs::Dir;

use super::{open_dir_nofollow, FilesystemError};

/// The directory `names` reach beneath `dir`, a link on the way refused as
/// a loop, walked in one call as 9p's `Twalk` walks several names.
pub(super) fn walk(dir: &Dir, names: &[String]) -> Result<Dir, FilesystemError> {
    if let Some(opened) = kernel::walk(dir, names) {
        return opened;
    }
    by_name(dir, names)
}

/// Each name opened in turn, the way every host can.
fn by_name(dir: &Dir, names: &[String]) -> Result<Dir, FilesystemError> {
    let (first, rest) = names.split_first().ok_or(FilesystemError::InvalidInput)?;
    rest.iter()
        .try_fold(open_dir_nofollow(dir, first)?, |dir, name| {
            open_dir_nofollow(&dir, name)
        })
}

/// What the host's errno names, a link on the way being a loop and a path
/// leading out of `dir` not permitted.
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn to_error(errno: i32) -> FilesystemError {
    const LOOP: i32 = rustix::io::Errno::LOOP.raw_os_error();
    #[cfg(target_os = "linux")]
    const LEADS_OUT: i32 = rustix::io::Errno::XDEV.raw_os_error();
    #[cfg(target_os = "macos")]
    const LEADS_OUT: i32 = libc::ENOTCAPABLE;
    match errno {
        LOOP => FilesystemError::FilesystemLoop,
        LEADS_OUT => FilesystemError::NotPermitted,
        _ => super::to_error(std::io::Error::from_raw_os_error(errno)),
    }
}

#[cfg(target_os = "linux")]
mod kernel {
    use std::fs::File;
    use std::sync::atomic::{AtomicBool, Ordering};

    use cap_std::fs::Dir;
    use rustix::fs::{openat2, Mode, OFlags, ResolveFlags};
    use rustix::io::Errno;

    use super::{to_error, FilesystemError};

    /// How often a resolution `openat2` asks to retry is retried before
    /// it fails, as a rename elsewhere can keep it from finishing.
    const RETRIES: usize = 8;

    /// Whether this kernel has `openat2`, which arrived in Linux 5.6.
    static SUPPORTED: AtomicBool = AtomicBool::new(true);

    pub(super) fn walk(dir: &Dir, names: &[String]) -> Option<Result<Dir, FilesystemError>> {
        if !SUPPORTED.load(Ordering::Relaxed) {
            return None;
        }
        let path = names.join("/");
        let mut answer = Errno::AGAIN;
        for _ in 0..RETRIES {
            match openat2(
                dir,
                &path,
                OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
                Mode::empty(),
                ResolveFlags::BENEATH | ResolveFlags::NO_SYMLINKS | ResolveFlags::NO_MAGICLINKS,
            ) {
                Ok(fd) => return Some(Ok(Dir::from_std_file(File::from(fd)))),
                Err(Errno::AGAIN) => continue,
                Err(Errno::NOSYS) => {
                    SUPPORTED.store(false, Ordering::Relaxed);
                    return None;
                }
                Err(errno) => {
                    answer = errno;
                    break;
                }
            }
        }
        Some(Err(to_error(answer.raw_os_error())))
    }
}

#[cfg(target_os = "macos")]
mod kernel {
    use std::ffi::CString;
    use std::fs::File;
    use std::io;
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::sync::OnceLock;

    use cap_std::fs::Dir;

    use super::{to_error, FilesystemError};

    /// `O_RESOLVE_BENEATH` as `sys/fcntl.h` numbers it; the libc crate
    /// names no such flag for macOS.
    const O_RESOLVE_BENEATH: libc::c_int = 0x1000;

    /// The first Darwin release of macOS 26, below which an older kernel
    /// could ignore a flag it does not know and follow a link.
    const DARWIN_OF_MACOS_26: u32 = 25;

    fn supported() -> bool {
        static SUPPORTED: OnceLock<bool> = OnceLock::new();
        *SUPPORTED.get_or_init(|| {
            rustix::system::uname()
                .release()
                .to_str()
                .ok()
                .and_then(|release| release.split('.').next()?.parse::<u32>().ok())
                .is_some_and(|major| major >= DARWIN_OF_MACOS_26)
        })
    }

    pub(super) fn walk(dir: &Dir, names: &[String]) -> Option<Result<Dir, FilesystemError>> {
        if !supported() {
            return None;
        }
        let Ok(path) = CString::new(names.join("/")) else {
            return Some(Err(FilesystemError::InvalidInput));
        };
        // SAFETY: `path` is a NUL-terminated string that outlives the call,
        // and `dir` holds its descriptor open across it.
        let fd = unsafe {
            libc::openat(
                dir.as_raw_fd(),
                path.as_ptr(),
                libc::O_RDONLY
                    | libc::O_DIRECTORY
                    | libc::O_CLOEXEC
                    | libc::O_NOFOLLOW_ANY
                    | O_RESOLVE_BENEATH,
            )
        };
        if fd < 0 {
            let errno = io::Error::last_os_error().raw_os_error().unwrap_or(0);
            return Some(Err(to_error(errno)));
        }
        // SAFETY: `openat` just returned `fd`, which nothing else owns.
        Some(Ok(Dir::from_std_file(unsafe { File::from_raw_fd(fd) })))
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
mod kernel {
    use cap_std::fs::Dir;

    use super::FilesystemError;

    pub(super) fn walk(_dir: &Dir, _names: &[String]) -> Option<Result<Dir, FilesystemError>> {
        None
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use cap_std::ambient_authority;

    use super::*;

    /// A host tree of the test's own: `a/b/c`, a file `a/file`, and a link
    /// `a/link` to `b`, removed when it drops.
    struct Tree(PathBuf);

    impl Tree {
        fn new(name: &str) -> Self {
            let path =
                std::env::temp_dir().join(format!("kobako-beneath-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(path.join("a/b/c")).expect("the tree must be made");
            fs::write(path.join("a/file"), "x").expect("the file must be made");
            #[cfg(unix)]
            std::os::unix::fs::symlink("b", path.join("a/link")).expect("the link must be made");
            Self(path)
        }

        fn dir(&self) -> Dir {
            Dir::open_ambient_dir(&self.0, ambient_authority()).expect("the tree must open")
        }
    }

    impl Drop for Tree {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn names(path: &str) -> Vec<String> {
        path.split('/').map(str::to_string).collect()
    }

    /// What `open` answers for `path`, only whether it opened kept.
    fn answer(
        open: impl Fn(&Dir, &[String]) -> Result<Dir, FilesystemError>,
        dir: &Dir,
        path: &str,
    ) -> Result<(), FilesystemError> {
        open(dir, &names(path)).map(drop)
    }

    // The host's kernel path and the one-name-at-a-time path must answer
    // alike, since which one runs depends on the host.
    #[test]
    fn the_kernel_and_each_name_answer_alike() {
        let tree = Tree::new("alike");
        let dir = tree.dir();
        let mut paths = vec!["a/b/c", "a/missing/c", "a/file/c", "a/file"];
        if cfg!(unix) {
            paths.extend(["a/link/c", "a/link"]);
        }
        for path in paths {
            assert_eq!(
                answer(walk, &dir, path),
                answer(by_name, &dir, path),
                "{path} opened beneath a directory by the kernel must answer as each name opened \
                 in turn answers"
            );
        }
    }

    #[test]
    fn a_link_on_the_way_is_a_loop() {
        if !cfg!(unix) {
            return;
        }
        let tree = Tree::new("link");
        assert_eq!(
            answer(walk, &tree.dir(), "a/link/c"),
            Err(FilesystemError::FilesystemLoop),
            "a/link/c opened beneath a directory, through a link, must be refused as a loop"
        );
    }

    #[test]
    fn a_path_reaches_its_directory() {
        let tree = Tree::new("reach");
        let opened = walk(&tree.dir(), &names("a/b")).expect("a/b must open");
        assert!(
            opened.open_dir("c").is_ok(),
            "a/b opened beneath a directory must be the directory holding c"
        );
    }
}
