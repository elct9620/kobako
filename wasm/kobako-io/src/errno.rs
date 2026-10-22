//! `IOError`, `EOFError`, `SystemCallError`, and the `Errno` classes a
//! file operation raises, mirroring CRuby's hierarchy with mruby-errno's
//! wording: `Errno::ENOENT` reads "No such file or directory - path".

use std::io;

use beni::prelude::*;
use beni::value::qnil;
use beni::{Error, Exception, ExceptionClass, IntoValue, Mrb, Value};

/// One `Errno` class: its name, the WASI errno it answers, the number
/// CRuby on Linux gives it as `Errno`, and its message.
struct Code {
    name: &'static core::ffi::CStr,
    wasi: i32,
    linux: i32,
    message: &'static str,
}

const fn code(
    name: &'static core::ffi::CStr,
    wasi: i32,
    linux: i32,
    message: &'static str,
) -> Code {
    Code {
        name,
        wasi,
        linux,
        message,
    }
}

const CODES: &[Code] = &[
    code(c"EACCES", 2, 13, "Permission denied"),
    code(c"EBADF", 8, 9, "Bad file descriptor"),
    code(c"EBUSY", 10, 16, "Device or resource busy"),
    code(c"EDEADLK", 16, 35, "Resource deadlock avoided"),
    code(c"EEXIST", 20, 17, "File exists"),
    code(
        c"EILSEQ",
        25,
        84,
        "Invalid or incomplete multibyte or wide character",
    ),
    code(c"EINTR", 27, 4, "Interrupted system call"),
    code(c"EINVAL", 28, 22, "Invalid argument"),
    code(c"EIO", 29, 5, "Input/output error"),
    code(c"EISDIR", 31, 21, "Is a directory"),
    code(c"ELOOP", 32, 40, "Too many levels of symbolic links"),
    code(c"EMFILE", 33, 24, "Too many open files"),
    code(c"ENAMETOOLONG", 37, 36, "File name too long"),
    code(c"ENOENT", 44, 2, "No such file or directory"),
    code(c"ENOMEM", 48, 12, "Cannot allocate memory"),
    code(c"ENOSPC", 51, 28, "No space left on device"),
    code(c"ENOTDIR", 54, 20, "Not a directory"),
    code(c"ENOTEMPTY", 55, 39, "Directory not empty"),
    code(c"ENOTSOCK", 57, 88, "Socket operation on non-socket"),
    code(c"ENOTSUP", 58, 95, "Operation not supported"),
    code(
        c"EOVERFLOW",
        61,
        75,
        "Value too large for defined data type",
    ),
    code(c"EPERM", 63, 1, "Operation not permitted"),
    code(c"EROFS", 69, 30, "Read-only file system"),
    code(c"ESPIPE", 70, 29, "Illegal seek"),
    code(c"EXDEV", 75, 18, "Invalid cross-device link"),
];

/// WASI's `ENOTCAPABLE`: a path outside every mounted filesystem. CRuby
/// names no such class, so it is raised as the denial it is.
const WASI_NOTCAPABLE: i32 = 76;

pub(crate) fn init(mrb: &Mrb) -> Result<(), Error> {
    let object = mrb.class_object();
    let io_error = object.define_error(mrb, c"IOError", mrb.exception_standard_error())?;
    object.define_error(mrb, c"EOFError", io_error)?;
    let system_call =
        object.define_error(mrb, c"SystemCallError", mrb.exception_standard_error())?;
    system_call.define_singleton_method(mrb, c"new", beni::method!(sce_s_new, -1))?;
    system_call.define_singleton_method(mrb, c"exception", beni::method!(sce_s_new, -1))?;
    system_call.define_method(mrb, c"errno", beni::method!(sce_errno, 0))?;
    let errno = mrb.define_module(c"Errno")?;
    for code in CODES {
        errno
            .define_error(mrb, code.name, system_call)?
            .const_set(mrb, c"Errno", code.linux)?;
    }
    Ok(())
}

/// The `Errno` number a class stands for; `SystemCallError` itself, whose
/// `Errno` lookup reaches the module, stands for none.
fn number_of(mrb: &Mrb, class: Value) -> Option<i32> {
    ExceptionClass::from_value(class)?
        .const_get::<_, Value>(mrb, c"Errno")
        .ok()
        .and_then(i32::from_value)
}

/// `SystemCallError.new(message = nil, errno = nil)` and each `Errno`
/// class's `new(message = nil)`, as CRuby builds them: the message
/// leads with the code's own, and a known number on `SystemCallError`
/// answers its `Errno` class.
fn sce_s_new(mrb: &Mrb, self_: Value, args: &[Value]) -> Result<Value, Error> {
    let own = number_of(mrb, self_);
    let (message, number) = match (own, args) {
        (Some(own), [message] | [message, ..]) => (Some(*message), Some(own)),
        (Some(own), []) => (None, Some(own)),
        (None, [number]) if i32::from_value(*number).is_some() => (None, i32::from_value(*number)),
        (None, [message, number, ..]) => (Some(*message), i32::from_value(*number)),
        (None, [message]) => (Some(*message), None),
        (None, []) => (None, None),
    };
    let message = message.filter(|message| !message.is_nil());
    let known = number.and_then(|number| CODES.iter().find(|code| code.linux == number));
    let class = match (own, known) {
        (None, Some(code)) => errno_class(mrb, code.name)?,
        _ => ExceptionClass::from_value(self_).ok_or_else(|| type_error(mrb, self_))?,
    };
    let mut text = match (known, number) {
        (Some(code), _) => code.message.to_string(),
        (None, Some(number)) => format!("Unknown error: {number}"),
        (None, None) => "unknown error".to_string(),
    };
    if let Some(message) = message {
        let message = String::from_value(message).ok_or_else(|| type_error(mrb, message))?;
        text = format!("{text} - {message}");
    }
    let exception = class.new_str(mrb, mrb.str_new(text.as_bytes()));
    if let (None, Some(number)) = (known, number) {
        exception.ivar_set(mrb, c"@errno", number)?;
    }
    Ok(exception.as_value())
}

/// The number the error's class stands for, or the one it was built with.
fn sce_errno(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    let class = self_.funcall(mrb, c"class", ())?;
    if let Some(number) = number_of(mrb, class) {
        return Ok(number.into_value(mrb));
    }
    match Exception::from_value(self_) {
        Some(exception) => exception.ivar_get(mrb, c"@errno"),
        None => Ok(qnil().as_value()),
    }
}

fn type_error(mrb: &Mrb, value: Value) -> Error {
    raised(
        mrb,
        mrb.exception_type_error(),
        &format!(
            "no implicit conversion of {} into String",
            value.classname(mrb)
        ),
    )
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

/// WASI's `EBADF` on a file this guest opened: the file was not opened
/// for the transfer asked of it.
const WASI_BADF: i32 = 8;

/// The error for `err` from reading or writing, `purpose`, a file this
/// guest opened: one not opened for it is an `IOError`, as CRuby raises.
pub(crate) fn from_transfer(mrb: &Mrb, err: &io::Error, path: &str, purpose: &str) -> Error {
    if err.raw_os_error() == Some(WASI_BADF) {
        return io_error(mrb, &format!("not opened for {purpose}"));
    }
    from_io(mrb, err, path)
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
        Some(code) => named(mrb, code.name, path),
        None => io_error(mrb, &format!("{err} - {path}")),
    }
}

/// The `Errno` class `name` names, from an operation on `path`.
pub(crate) fn named(mrb: &Mrb, name: &core::ffi::CStr, path: &str) -> Error {
    let message = CODES
        .iter()
        .find(|code| code.name == name)
        .map_or("unknown error", |code| code.message);
    raised(mrb, errno_class(mrb, name), &format!("{message} - {path}"))
}
