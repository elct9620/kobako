//! Host links the preview1 tests stage beneath a Bind Mount.

use std::path::Path;

/// Make a host link at `at` leading to `target`, spelled as a unix path.
/// Windows makes a link to a directory apart from one to a file, so there
/// the kind follows what `target` names now, a file where nothing stands,
/// and `target` takes the host's separator.
pub(super) fn symlink(target: impl AsRef<Path>, at: impl AsRef<Path>) {
    let (target, at) = (target.as_ref(), at.as_ref());
    #[cfg(unix)]
    let made = std::os::unix::fs::symlink(target, at);
    #[cfg(windows)]
    let made = {
        let target = std::path::PathBuf::from(target.to_string_lossy().replace('/', "\\"));
        let beside = at.parent().unwrap_or(Path::new(""));
        if beside.join(&target).is_dir() {
            std::os::windows::fs::symlink_dir(&target, at)
        } else {
            std::os::windows::fs::symlink_file(&target, at)
        }
    };
    made.unwrap_or_else(|err| {
        panic!(
            "the host must make a link at {} to {}: {err}; a Windows host makes one only \
             in Developer Mode or with the privilege to create symbolic links",
            at.display(),
            target.display()
        )
    });
}
