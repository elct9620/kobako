//! `IOError`, `EOFError`, `SystemCallError`, and the `Errno` classes a
//! file operation raises, mirroring CRuby's hierarchy with mruby-errno's
//! wording: `Errno::ENOENT` reads "No such file or directory - path".

use std::io;

use beni::prelude::*;
use beni::{Error, ExceptionClass, Mrb};

/// One `Errno` class: its name, the WASI errno it answers, its message.
struct Code {
    name: &'static core::ffi::CStr,
    wasi: i32,
    message: &'static str,
}

const CODES: &[Code] = &[
    Code {
        name: c"EACCES",
        wasi: 2,
        message: "Permission denied",
    },
    Code {
        name: c"EBADF",
        wasi: 8,
        message: "Bad file descriptor",
    },
    Code {
        name: c"EEXIST",
        wasi: 20,
        message: "File exists",
    },
    Code {
        name: c"EINVAL",
        wasi: 28,
        message: "Invalid argument",
    },
    Code {
        name: c"EIO",
        wasi: 29,
        message: "Input/output error",
    },
    Code {
        name: c"EISDIR",
        wasi: 31,
        message: "Is a directory",
    },
    Code {
        name: c"EMFILE",
        wasi: 33,
        message: "Too many open files",
    },
    Code {
        name: c"ENAMETOOLONG",
        wasi: 37,
        message: "File name too long",
    },
    Code {
        name: c"ENOENT",
        wasi: 44,
        message: "No such file or directory",
    },
    Code {
        name: c"ENOSPC",
        wasi: 51,
        message: "No space left on device",
    },
    Code {
        name: c"ENOTDIR",
        wasi: 54,
        message: "Not a directory",
    },
    Code {
        name: c"ENOTEMPTY",
        wasi: 55,
        message: "Directory not empty",
    },
    Code {
        name: c"ENOTSUP",
        wasi: 58,
        message: "Operation not supported",
    },
    Code {
        name: c"EROFS",
        wasi: 69,
        message: "Read-only file system",
    },
];

/// WASI's `ENOTCAPABLE`: a path outside every mounted filesystem. CRuby
/// names no such class, so it is raised as the denial it is.
const WASI_NOTCAPABLE: i32 = 76;

pub(crate) fn init(mrb: &Mrb) -> Result<(), Error> {
    let object = mrb.object_class();
    let io_error = object.define_error(mrb, c"IOError", mrb.exception_standard_error())?;
    object.define_error(mrb, c"EOFError", io_error)?;
    let system_call =
        object.define_error(mrb, c"SystemCallError", mrb.exception_standard_error())?;
    let errno = mrb.define_module(c"Errno")?;
    for code in CODES {
        errno.define_error(mrb, code.name, system_call)?;
    }
    Ok(())
}

fn errno_class(mrb: &Mrb, name: &core::ffi::CStr) -> Result<ExceptionClass, Error> {
    mrb.module_get(c"Errno")?.const_get(mrb, name)
}

fn raised(mrb: &Mrb, class: Result<ExceptionClass, Error>, message: &str) -> Error {
    match class {
        Ok(class) => Error::new(mrb, class, message),
        Err(err) => err,
    }
}

pub(crate) fn io_error(mrb: &Mrb, message: &str) -> Error {
    raised(mrb, mrb.exc_get(c"IOError"), message)
}

pub(crate) fn not_implemented(mrb: &Mrb, message: &str) -> Error {
    raised(mrb, mrb.exception_not_imp_error(), message)
}

/// The error for `err` from an operation on `path`, as the `Errno` class
/// the failure names, or an `IOError` for one no class names.
pub(crate) fn from_io(mrb: &Mrb, err: &io::Error, path: &str) -> Error {
    let raw = err
        .raw_os_error()
        .map(|raw| if raw == WASI_NOTCAPABLE { 2 } else { raw });
    let code = CODES
        .iter()
        .find(|code| Some(code.wasi) == raw)
        .or_else(|| {
            let name: &core::ffi::CStr = match err.kind() {
                io::ErrorKind::NotFound => c"ENOENT",
                io::ErrorKind::PermissionDenied => c"EACCES",
                io::ErrorKind::AlreadyExists => c"EEXIST",
                io::ErrorKind::IsADirectory => c"EISDIR",
                io::ErrorKind::NotADirectory => c"ENOTDIR",
                io::ErrorKind::DirectoryNotEmpty => c"ENOTEMPTY",
                io::ErrorKind::ReadOnlyFilesystem => c"EROFS",
                io::ErrorKind::InvalidInput => c"EINVAL",
                io::ErrorKind::Unsupported => c"ENOTSUP",
                _ => return None,
            };
            CODES.iter().find(|code| code.name == name)
        });
    match code {
        Some(code) => raised(
            mrb,
            errno_class(mrb, code.name),
            &format!("{} - {path}", code.message),
        ),
        None => io_error(mrb, &format!("{err} - {path}")),
    }
}
