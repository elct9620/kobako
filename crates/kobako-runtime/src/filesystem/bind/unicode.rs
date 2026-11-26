//! How a guest's names and a host's spell each other, as Linux's CIFS client
//! converts in `cifs_unicode.c`. A guest name reaches the host only through
//! `to_host_name`, and a host name or link target reaches the guest only
//! through `to_guest_name` or `to_guest_target`, so a spelling a host reads
//! apart from Linux is judged here alone.
//!
//! | What the host spells apart | Toward | Answer |
//! |---|---|---|
//! | `\`, `:`, a wildcard, a control character, a device's name, or a trailing `.` or space in a name, on Windows | host | refused as invalid input |
//! | a name no UTF-8 spells | guest | refused as an illegal byte sequence |
//! | `\` between a link target's names, and its drive or share, on Windows | guest | `/`, and the path rooted at `/` |
//!
//! A spelling the host folds to a stored name is the lookup's to answer.

use std::ffi::OsString;

use super::Errno;

/// `name` as `cap_std` takes it. Windows reads some names as other names or
/// as devices, so there one is refused rather than reached as an entry a
/// Mount's Policy never saw.
pub(super) fn to_host_name(name: &str) -> Result<&str, Errno> {
    if cfg!(windows) && is_windows_alias(name) {
        return Err(Errno::InvalidInput);
    }
    Ok(name)
}

/// A name the host holds, as a guest name spells it.
pub(super) fn to_guest_name(name: OsString) -> Result<String, Errno> {
    name.into_string().map_err(|_| Errno::IllegalSequence)
}

/// A link's `target` as a guest path spells it. Windows separates names
/// with `\` and roots an absolute path at a drive or a share, so there each
/// `\` reads as `/` and the drive or share drops, leaving a path rooted at
/// `/` as on unix.
pub(super) fn to_guest_target(target: OsString) -> Result<String, Errno> {
    let target = to_guest_name(target)?;
    #[cfg(windows)]
    {
        use std::path::{Component, Path};
        let prefix = match Path::new(&target).components().next() {
            Some(Component::Prefix(prefix)) => prefix.as_os_str().len(),
            _ => 0,
        };
        Ok(target[prefix..].replace('\\', "/"))
    }
    #[cfg(not(windows))]
    Ok(target)
}

/// Whether Windows reads `name` as something else: a `\` as a separator, a `:`
/// as a stream of a file, a trailing `.` or space as nothing, a wildcard as a
/// pattern, or a device's name, whatever extension follows, as the device.
fn is_windows_alias(name: &str) -> bool {
    const DEVICES: [&str; 33] = [
        "CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$", "CLOCK$", "COM0", "COM1", "COM2", "COM3",
        "COM4", "COM5", "COM6", "COM7", "COM8", "COM9", "COM¹", "COM²", "COM³", "LPT0", "LPT1",
        "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9", "LPT¹", "LPT²", "LPT³",
    ];
    let reserved = |c: char| c.is_ascii_control() || "\\:*?\"<>|".contains(c);
    let stem = name.split('.').next().unwrap_or(name).trim_end();
    name.contains(reserved)
        || name.ends_with(['.', ' '])
        || DEVICES.contains(&stem.to_uppercase().as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Judged on every host, as only Windows's own CI reaches the refusal, and
    // what the host would do with each name is what no other host can show.
    // @behavior FS-153
    #[test]
    fn a_name_windows_reads_as_another_is_told_apart() {
        let aliases = [
            "secret.",
            "secret ",
            "file:stream",
            r"d\f",
            "con",
            "NUL.txt",
            "conin$",
            "com1 .log",
            "a?",
            "a*",
            "a<",
            "a|",
            "a\"",
            "a\u{1}",
        ];
        let plain = ["secret", "file.txt", "console", "com10", ".hidden", "a.b.c"];
        assert_eq!(
            (
                aliases
                    .iter()
                    .filter(|name| !is_windows_alias(name))
                    .count(),
                plain.iter().filter(|name| is_windows_alias(name)).count(),
            ),
            (0, 0),
            "a name a Windows host reads as another name or as a device must be told apart \
             from a plain name, so a Bind there refuses it, and no plain name may be taken \
             for one"
        );
    }
}
