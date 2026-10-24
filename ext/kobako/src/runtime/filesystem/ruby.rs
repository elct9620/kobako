//! A Filesystem a Host App writes in Ruby: any object answering `open`
//! and `stat`, asked through `gvl::reenter` on every call the guest makes.
//!
//! Its methods take the names and shapes of Ruby's own `File`, `Dir`, and
//! `IO`, so a Host App can hand an open call on to a real `::File`. A
//! change method the object lacks answers as read-only; any other method
//! it lacks answers as unsupported. A file's `EOFError` from `pread` is
//! its end, an `Errno` the guest has a class for reaches the guest as
//! that code, and any other `StandardError` reaches the guest as `EIO`.

use magnus::value::{Opaque, ReprValue};
use magnus::{kwargs, prelude::*, Error as MagnusError, RClass, RModule, RString, Ruby, Value};

use kobako_runtime::filesystem::{
    DirEntry, File, FileType, Filesystem, FilesystemError, Metadata, OpenOptions, Rename,
};

use crate::runtime::pinned_value::PinnedValue;
use crate::runtime::{escape, gvl};

/// The Ruby object behind a Mount or a Layer. Whatever holds it on the
/// Ruby side keeps it alive and in place: the invocation pinning its
/// Mounts, or the Overlay marking its Layers.
pub(crate) struct RubyFilesystem {
    object: Opaque<Value>,
}

impl RubyFilesystem {
    /// The object as a Filesystem, when it answers `open` and `stat`.
    pub(crate) fn from_value(value: Value) -> Option<Self> {
        let answers = |name| value.respond_to(name, false).unwrap_or(false);
        (answers("open") && answers("stat")).then(|| Self {
            object: Opaque::from(value),
        })
    }
}

/// Ask `object` its `name` method with the GVL held, `answer` making the
/// call and reading what it returns before Ruby runs anything else. An
/// object lacking the method answers `missing`.
fn ask<T>(
    object: Opaque<Value>,
    name: &str,
    missing: FilesystemError,
    answer: impl FnOnce(&Ruby, Value) -> Result<T, MagnusError>,
) -> Result<T, FilesystemError> {
    gvl::reenter(|| {
        let ruby = Ruby::get().expect("Ruby handle unavailable in a Filesystem call");
        let object: Value = ruby.get_inner(object);
        if !object.respond_to(name, false).unwrap_or(false) {
            return Err(missing);
        }
        answer(&ruby, object).map_err(|err| match escape::hold(&ruby, err) {
            Err(err) => to_error(err),
            // The invocation ends before the guest reads this.
            Ok(()) => FilesystemError::Other(String::new()),
        })
    })
}

/// Ask `object` a change, answering nothing more than whether it was made.
fn change(
    object: Opaque<Value>,
    name: &str,
    args: impl magnus::ArgList,
    missing: FilesystemError,
) -> Result<(), FilesystemError> {
    ask(object, name, missing, |_, object| {
        object.funcall::<_, _, Value>(name, args).map(drop)
    })
}

/// The FilesystemError the nearest `Errno` class of `err` stands for, by
/// its POSIX name; anything else is `Other`, which reaches the guest as
/// `EIO`.
fn to_error(err: MagnusError) -> FilesystemError {
    let named = err.value().and_then(|exception| {
        exception
            .class()
            .ancestors()
            .into_iter()
            .find_map(|module| {
                let name: Option<String> = module.funcall("name", ()).ok()?;
                FilesystemError::from_posix_name(name?.strip_prefix("Errno::")?)
            })
    });
    named.unwrap_or_else(|| FilesystemError::Other(err.to_string()))
}

/// `File::Constants` flags for `options`, read from Ruby since their
/// values differ by platform.
fn to_flags(ruby: &Ruby, options: &OpenOptions) -> Result<i64, MagnusError> {
    let constants: RModule = ruby
        .class_object()
        .const_get::<_, RClass>("File")?
        .const_get("Constants")?;
    let flag = |name| constants.const_get::<_, i64>(name);
    let mut flags = match (options.read, options.write || options.truncate) {
        (_, false) => flag("RDONLY")?,
        (false, true) => flag("WRONLY")?,
        (true, true) => flag("RDWR")?,
    };
    if options.create || options.create_new {
        flags |= flag("CREAT")?;
    }
    if options.create_new {
        flags |= flag("EXCL")?;
    }
    if options.truncate {
        flags |= flag("TRUNC")?;
    }
    if options.nofollow {
        flags |= flag("NOFOLLOW")?;
    }
    Ok(flags)
}

fn join(dir: &str, name: &str) -> String {
    if dir.is_empty() {
        name.to_string()
    } else {
        format!("{dir}/{name}")
    }
}

impl Filesystem for RubyFilesystem {
    fn open(&self, path: &str, options: &OpenOptions) -> Result<Box<dyn File>, FilesystemError> {
        ask(
            self.object,
            "open",
            FilesystemError::Unsupported,
            |ruby, fs| {
                let file: Value = fs.funcall("open", (path, to_flags(ruby, options)?))?;
                Ok(Box::new(RubyFile::pin(ruby, file)) as Box<dyn File>)
            },
        )
    }

    fn metadata(&self, path: &str) -> Result<Metadata, FilesystemError> {
        ask(
            self.object,
            "stat",
            FilesystemError::Unsupported,
            |_, fs| {
                let stat: Value = fs.funcall("stat", (path,))?;
                if stat.funcall::<_, _, bool>("directory?", ())? {
                    return Ok(Metadata::new(FileType::Directory, 0));
                }
                Ok(Metadata::new(FileType::File, stat.funcall("size", ())?))
            },
        )
    }

    fn read_dir(&self, path: &str) -> Result<Vec<DirEntry>, FilesystemError> {
        let names: Vec<String> = ask(
            self.object,
            "children",
            FilesystemError::Unsupported,
            |_, fs| fs.funcall("children", (path,)),
        )?;
        names
            .into_iter()
            .map(|name| {
                let file_type = self.metadata(&join(path, &name))?.file_type;
                Ok(DirEntry::new(name, file_type))
            })
            .collect()
    }

    fn create_dir(&self, path: &str) -> Result<(), FilesystemError> {
        change(
            self.object,
            "mkdir",
            (path,),
            FilesystemError::ReadOnlyFilesystem,
        )
    }

    fn remove_dir(&self, path: &str) -> Result<(), FilesystemError> {
        change(
            self.object,
            "rmdir",
            (path,),
            FilesystemError::ReadOnlyFilesystem,
        )
    }

    fn remove_file(&self, path: &str) -> Result<(), FilesystemError> {
        change(
            self.object,
            "unlink",
            (path,),
            FilesystemError::ReadOnlyFilesystem,
        )
    }

    /// `rename(from, to, directory:)`: `true` moves only a directory,
    /// `false` only a file, `nil` either. What stands at `from` meanwhile
    /// is the object's to keep still.
    fn rename(&self, from: &str, to: &str, rename: Rename) -> Result<(), FilesystemError> {
        let directory = match rename {
            Rename::File => Some(false),
            Rename::Directory => Some(true),
            _ => None,
        };
        change(
            self.object,
            "rename",
            (from, to, kwargs!("directory" => directory)),
            FilesystemError::ReadOnlyFilesystem,
        )
    }
}

/// One file the Ruby object opened, which only the guest's descriptor
/// refers to.
struct RubyFile {
    file: PinnedValue,
    closed: bool,
}

impl RubyFile {
    fn pin(ruby: &Ruby, file: Value) -> Self {
        Self {
            file: PinnedValue::new(ruby, file),
            closed: false,
        }
    }

    /// Ask the file to close, a file lacking `close` having nothing to
    /// release.
    fn release(&mut self) -> Result<(), FilesystemError> {
        self.closed = true;
        match change(self.file.get(), "close", (), FilesystemError::Unsupported) {
            Err(FilesystemError::Unsupported) => Ok(()),
            answer => answer,
        }
    }
}

impl File for RubyFile {
    fn read_at(&mut self, buf: &mut [u8], offset: u64) -> Result<usize, FilesystemError> {
        ask(
            self.file.get(),
            "pread",
            FilesystemError::Unsupported,
            |ruby, file| {
                // `IO#pread` raises `EOFError` at the end of a file, which reads as nothing.
                let read: Option<RString> = match file.funcall("pread", (buf.len(), offset)) {
                    Err(err) if err.is_kind_of(ruby.exception_eof_error()) => None,
                    read => read?,
                };
                let Some(read) = read else { return Ok(0) };
                // SAFETY: the bytes are copied out before Ruby runs again.
                let bytes = unsafe { read.as_slice() };
                let len = bytes.len().min(buf.len());
                buf[..len].copy_from_slice(&bytes[..len]);
                Ok(len)
            },
        )
    }

    fn write_at(&mut self, buf: &[u8], offset: u64) -> Result<usize, FilesystemError> {
        ask(
            self.file.get(),
            "pwrite",
            FilesystemError::ReadOnlyFilesystem,
            |ruby, file| file.funcall("pwrite", (ruby.str_from_slice(buf), offset)),
        )
        .and_then(|written: usize| match written {
            written if written <= buf.len() => Ok(written),
            written => Err(FilesystemError::Other(format!(
                "pwrite answered {written} bytes written of the {} it was handed",
                buf.len()
            ))),
        })
    }

    fn metadata(&self) -> Result<Metadata, FilesystemError> {
        ask(
            self.file.get(),
            "size",
            FilesystemError::Unsupported,
            |_, file| Ok(Metadata::new(FileType::File, file.funcall("size", ())?)),
        )
    }

    fn set_len(&mut self, len: u64) -> Result<(), FilesystemError> {
        change(
            self.file.get(),
            "truncate",
            (len,),
            FilesystemError::ReadOnlyFilesystem,
        )
    }

    /// A file lacking `fsync` has nothing of its own to flush.
    fn sync(&mut self) -> Result<(), FilesystemError> {
        match change(self.file.get(), "fsync", (), FilesystemError::Unsupported) {
            Err(FilesystemError::Unsupported) => Ok(()),
            answer => answer,
        }
    }

    fn close(mut self: Box<Self>) -> Result<(), FilesystemError> {
        self.release()
    }
}

/// A file the guest never closed is closed here, its failure discarded.
impl Drop for RubyFile {
    fn drop(&mut self) {
        if !self.closed {
            let _ = self.release();
        }
    }
}
