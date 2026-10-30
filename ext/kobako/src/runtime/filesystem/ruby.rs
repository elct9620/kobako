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
use magnus::{prelude::*, Error as MagnusError, RClass, RModule, RString, Ruby, Value};

use std::any::Any;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use kobako_runtime::filesystem::{
    DirEntry, FileOperations, FileType, FilesystemError, InodeOperations, Lookup, Metadata,
    OpenOptions,
};
use kobako_runtime::vfs::{Dentry, Inode};

use crate::runtime::pinned_value::PinnedValue;
use crate::runtime::{escape, gvl};

/// The Ruby object behind a Mount or a Layer. Whatever holds it on the
/// Ruby side keeps it alive and in place: the invocation pinning its
/// Mounts, or the Overlay marking its Layers. The object answers by path,
/// so each call asks it by the path the dentry stands at now, as CIFS
/// builds one from the dentry, and a directory opened beneath it is the
/// same object.
pub(crate) struct RubyFilesystem {
    object: Opaque<Value>,
    /// The device a stat without a `dev` names, the object's own, shared by
    /// every directory opened beneath it.
    dev: u64,
}

impl RubyFilesystem {
    /// The object as a Filesystem, when it answers `open` and `stat`.
    pub(crate) fn from_value(value: Value) -> Option<Self> {
        let answers = |name| value.respond_to(name, false).unwrap_or(false);
        (answers("open") && answers("stat")).then(|| Self {
            object: Opaque::from(value),
            dev: Metadata::unique_device(),
        })
    }

    /// What the object's `stat` answers for `path`.
    fn stat(&self, path: String) -> Result<Metadata, FilesystemError> {
        let own_dev = self.dev;
        ask(
            self.object,
            "stat",
            FilesystemError::Unsupported,
            |_, fs| {
                let stat: Value = fs.funcall("stat", (path,))?;
                let file_type = if stat.funcall::<_, _, bool>("directory?", ())? {
                    FileType::Directory
                } else {
                    FileType::File
                };
                let metadata = Metadata::new(file_type, stat.funcall("size", ())?);
                // A stat names its identity as `File::Stat` does; without a
                // `dev` the Filesystem stands for a device of its own, so it
                // shares an identity with no other Filesystem.
                let dev = if stat.respond_to("dev", false)? {
                    stat.funcall("dev", ())?
                } else {
                    own_dev
                };
                let metadata = metadata
                    .with_identity(dev, stat.funcall("ino", ())?)
                    .with_times(
                        time_of(stat, "atime")?,
                        time_of(stat, "mtime")?,
                        time_of(stat, "ctime")?,
                    );
                Ok(if stat.respond_to("nlink", false)? {
                    metadata.with_nlink(stat.funcall("nlink", ())?)
                } else {
                    metadata
                })
            },
        )
    }
}

/// Ask `object` its `name` method with the GVL held, `answer` making the
/// call and reading what it returns before Ruby runs anything else. An
/// object lacking the method fails as `missing`.
fn ask<T>(
    object: Opaque<Value>,
    name: &str,
    missing: FilesystemError,
    answer: impl FnOnce(&Ruby, Value) -> Result<T, MagnusError>,
) -> Result<T, FilesystemError> {
    ask_or(object, name, Err(missing), answer)
}

/// As `ask`, an object lacking the method answering `missing`.
fn ask_or<T>(
    object: Opaque<Value>,
    name: &str,
    missing: Result<T, FilesystemError>,
    answer: impl FnOnce(&Ruby, Value) -> Result<T, MagnusError>,
) -> Result<T, FilesystemError> {
    gvl::reenter(|| {
        let ruby = Ruby::get().expect("Ruby handle unavailable in a Filesystem call");
        let object: Value = ruby.get_inner(object);
        if !object.respond_to(name, false).unwrap_or(false) {
            return missing;
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

/// Ask `object` a change it may lack, an object lacking `name` having
/// nothing of its own to change, while one that answers and refuses still
/// fails, whatever `Errno` it raises.
fn change_if_answered(object: Opaque<Value>, name: &str) -> Result<(), FilesystemError> {
    ask_or(object, name, Ok(()), |_, object| {
        object.funcall::<_, _, Value>(name, ()).map(drop)
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
    // The runtime follows a link itself, so no open follows one, wherever
    // the host's Ruby can say so: Windows defines no `NOFOLLOW`.
    if constants.funcall::<_, _, bool>("const_defined?", ("NOFOLLOW",))? {
        flags |= flag("NOFOLLOW")?;
    }
    Ok(flags)
}

/// The `Time` a stat answers to `name`, as `File::Stat#mtime` gives one;
/// `None` where it answers none, or a time before the Unix epoch.
fn time_of(stat: Value, name: &str) -> Result<Option<SystemTime>, MagnusError> {
    if !stat.respond_to(name, false)? {
        return Ok(None);
    }
    let time: Value = stat.funcall(name, ())?;
    let seconds: i64 = time.funcall("to_i", ())?;
    let nanoseconds: u32 = time.funcall("nsec", ())?;
    Ok(u64::try_from(seconds)
        .ok()
        .and_then(|seconds| UNIX_EPOCH.checked_add(Duration::new(seconds, nanoseconds))))
}

fn join(dir: &str, name: &str) -> String {
    if dir.is_empty() {
        name.to_string()
    } else {
        format!("{dir}/{name}")
    }
}

impl RubyFilesystem {
    fn open_at(
        &self,
        dentry: &Dentry,
        options: &OpenOptions,
    ) -> Result<Box<dyn FileOperations>, FilesystemError> {
        let path = dentry.path_raw();
        ask(
            self.object,
            "open",
            FilesystemError::Unsupported,
            |ruby, fs| {
                let file: Value = fs.funcall("open", (path, to_flags(ruby, options)?))?;
                Ok(Box::new(RubyFile::pin(ruby, file)) as Box<dyn FileOperations>)
            },
        )
    }

    fn entries_at(&self, dentry: &Dentry) -> Result<Vec<DirEntry>, FilesystemError> {
        let at = dentry.path_raw();
        let names: Vec<String> = ask(
            self.object,
            "children",
            FilesystemError::Unsupported,
            |_, fs| fs.funcall("children", (at.as_str(),)),
        )?;
        names
            .into_iter()
            .map(|name| {
                let file_type = self.stat(join(&at, &name))?.file_type;
                Ok(DirEntry::new(name, file_type))
            })
            .collect()
    }

    fn rename_to(
        &self,
        from: &Dentry,
        to_dir: &dyn InodeOperations,
        to: &Dentry,
    ) -> Result<(), FilesystemError> {
        (to_dir as &dyn Any)
            .downcast_ref::<Self>()
            .filter(|target| target.dev == self.dev)
            .ok_or(FilesystemError::CrossesDevices)?;
        change(
            self.object,
            "rename",
            (from.path_raw(), to.path_raw()),
            FilesystemError::ReadOnlyFilesystem,
        )
    }
}

/// Every entry answers through the same object, asked by the path a dentry
/// stands at, so a name always means what stands there now, the path a
/// Policy judges, and no `d_revalidate` is needed. A Ruby object answers
/// `stat` alone, so it holds no links to follow.
impl InodeOperations for RubyFilesystem {
    fn getattr(&self, dentry: &Dentry) -> Result<Metadata, FilesystemError> {
        self.stat(dentry.path_raw())
    }

    /// A Ruby object matches names exactly, as its contract asks. A lookup
    /// asks it nothing, so a walk through several names asks the object
    /// once, by the whole path, as 9p walks to a dentry's fid only when a
    /// call needs one.
    fn lookup(&self, _dentry: &Dentry) -> Option<Result<Lookup, FilesystemError>> {
        Some(Ok(Lookup::Found(Inode::by_path(Arc::new(Self {
            object: self.object,
            dev: self.dev,
        })))))
    }

    fn open(
        &self,
        dentry: &Dentry,
        options: &OpenOptions,
    ) -> Option<Result<Box<dyn FileOperations>, FilesystemError>> {
        Some(self.open_at(dentry, options))
    }

    fn create(
        &self,
        dentry: &Dentry,
        options: &OpenOptions,
    ) -> Option<Result<Box<dyn FileOperations>, FilesystemError>> {
        Some(self.open_at(dentry, options))
    }

    fn entries(&self, dentry: &Dentry) -> Option<Result<Vec<DirEntry>, FilesystemError>> {
        Some(self.entries_at(dentry))
    }

    fn mkdir(&self, dentry: &Dentry) -> Option<Result<(), FilesystemError>> {
        Some(change(
            self.object,
            "mkdir",
            (dentry.path_raw(),),
            FilesystemError::ReadOnlyFilesystem,
        ))
    }

    fn rmdir(&self, dentry: &Dentry) -> Option<Result<(), FilesystemError>> {
        Some(change(
            self.object,
            "rmdir",
            (dentry.path_raw(),),
            FilesystemError::ReadOnlyFilesystem,
        ))
    }

    fn unlink(&self, dentry: &Dentry) -> Option<Result<(), FilesystemError>> {
        Some(change(
            self.object,
            "unlink",
            (dentry.path_raw(),),
            FilesystemError::ReadOnlyFilesystem,
        ))
    }

    /// `rename(from, to)`, as `File.rename` takes it. What stands at `from`
    /// after the runtime checked it is the object's to keep still. Both
    /// directories are of one object, which their shared device names.
    fn rename(
        &self,
        from: &Dentry,
        to_dir: &dyn InodeOperations,
        to: &Dentry,
    ) -> Option<Result<(), FilesystemError>> {
        Some(self.rename_to(from, to_dir, to))
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
    fn close(&mut self) -> Result<(), FilesystemError> {
        self.closed = true;
        change_if_answered(self.file.get(), "close")
    }
}

impl FileOperations for RubyFile {
    fn read(&mut self, buf: &mut [u8], offset: u64) -> Result<usize, FilesystemError> {
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

    fn write(&mut self, buf: &[u8], offset: u64) -> Result<usize, FilesystemError> {
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

    /// The file's `append` writes all of `buf` at its end in one step and
    /// answers where the file then ends, as `IO#pos` does after a write to
    /// a file opened to append. A file lacking it cannot append, since its
    /// `size` and `pwrite` are two steps another writer can land between.
    fn append(&mut self, buf: &[u8]) -> Result<(u64, usize), FilesystemError> {
        ask(
            self.file.get(),
            "append",
            FilesystemError::Unsupported,
            |ruby, file| file.funcall("append", (ruby.str_from_slice(buf),)),
        )
        .and_then(|end: u64| match end.checked_sub(buf.len() as u64) {
            Some(at) => Ok((at, buf.len())),
            None => Err(FilesystemError::Other(format!(
                "append answered the file ending at {end}, before the {} bytes it wrote",
                buf.len()
            ))),
        })
    }

    fn getattr(&self) -> Result<Metadata, FilesystemError> {
        ask(
            self.file.get(),
            "size",
            FilesystemError::Unsupported,
            |_, file| Ok(Metadata::new(FileType::File, file.funcall("size", ())?)),
        )
    }

    fn truncate(&mut self, len: u64) -> Result<(), FilesystemError> {
        change(
            self.file.get(),
            "truncate",
            (len,),
            FilesystemError::ReadOnlyFilesystem,
        )
    }

    /// A file lacking `fsync` has nothing of its own to flush.
    fn fsync(&mut self) -> Result<(), FilesystemError> {
        change_if_answered(self.file.get(), "fsync")
    }

    fn release(mut self: Box<Self>) -> Result<(), FilesystemError> {
        self.close()
    }
}

/// A file the guest never closed is closed here, its failure discarded.
impl Drop for RubyFile {
    fn drop(&mut self) {
        if !self.closed {
            let _ = self.close();
        }
    }
}
