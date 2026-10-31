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
use std::sync::{Arc, Mutex, PoisonError, Weak};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use kobako_runtime::filesystem::{
    DentryOperations, DirEntry, FileOperations, FileType, FilesystemError, Iattr, InodeOperations,
    Kiocb, Lookup, Metadata, OpenOptions,
};
use kobako_runtime::vfs::{Dentry, Identity, Inode, NAME_MAX};

use crate::runtime::pinned_value::PinnedValue;
use crate::runtime::{escape, gvl};

/// The Ruby object behind a Mount or a Layer. Whatever holds it on the
/// Ruby side keeps it alive and in place: the invocation pinning its
/// Mounts, or the Overlay marking its Layers. The object answers by path,
/// so each call asks it by the path the dentry stands at now, as CIFS
/// builds one from the dentry, and a directory opened beneath it is the
/// same object.
#[derive(Clone, Copy)]
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
                let metadata = Metadata::new(kind_of(stat)?, stat.funcall("size", ())?);
                // A stat names its identity as `File::Stat` does; without a
                // `dev` the Filesystem stands for a device of its own, so it
                // shares an identity with no other Filesystem.
                let dev = if stat.respond_to("dev", false)? {
                    stat.funcall("dev", ())?
                } else {
                    own_dev
                };
                with_stat_record(stat, metadata.with_identity(dev, stat.funcall("ino", ())?))
            },
        )
    }
}

/// What a stat says its entry is. A `File::Stat` of a special file says so
/// through `ftype`, so it is judged as what it is rather than opened; a stat
/// reports no link, as a Ruby Filesystem holds none.
fn kind_of(stat: Value) -> Result<FileType, MagnusError> {
    if stat.funcall::<_, _, bool>("directory?", ())? {
        return Ok(FileType::Directory);
    }
    if !stat.respond_to("ftype", false)? {
        return Ok(FileType::File);
    }
    let ftype: String = stat.funcall("ftype", ())?;
    Ok(match ftype.as_str() {
        "fifo" => FileType::Fifo,
        "socket" => FileType::Socket,
        "blockSpecial" => FileType::BlockDevice,
        "characterSpecial" => FileType::CharDevice,
        _ => FileType::File,
    })
}

/// `metadata` with the times and the count of names `stat` records.
fn with_stat_record(stat: Value, metadata: Metadata) -> Result<Metadata, MagnusError> {
    let metadata = metadata.with_times(
        time_of(stat, "atime")?,
        time_of(stat, "mtime")?,
        time_of(stat, "ctime")?,
    );
    Ok(if stat.respond_to("nlink", false)? {
        metadata.with_nlink(stat.funcall("nlink", ())?)
    } else {
        metadata
    })
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
    /// Open the file `dentry` names and keep it on the inode, so the entry
    /// answers from it however its name changes, as 9p keeps an open fid.
    /// What this made is recorded first, as `->create` calls `d_instantiate`.
    fn open_at(
        &self,
        dentry: &Dentry,
        options: &OpenOptions,
    ) -> Result<Box<dyn FileOperations>, FilesystemError> {
        let path = dentry.path_raw();
        let file = ask(
            self.object,
            "open",
            FilesystemError::Unsupported,
            |ruby, fs| {
                let file: Value = fs.funcall("open", (path, to_flags(ruby, options)?))?;
                Ok(RubyFile::pin(ruby, file))
            },
        )?;
        if dentry.inode().is_none() {
            let made = self.stat(dentry.path_raw())?;
            dentry.add(Some(Inode::new(&made, Arc::new(*self))));
        }
        let inode = dentry.inode().ok_or(FilesystemError::NotFound)?;
        let kept = inode
            .private()
            .get_or_init(|| Arc::new(OpenRubyFile::default()))
            .clone();
        if let Ok(open) = kept.downcast::<OpenRubyFile>() {
            *open.0.lock().unwrap_or_else(PoisonError::into_inner) = Arc::downgrade(&file.file);
        }
        Ok(Box::new(file))
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
                let stat = self.stat(join(&at, &name))?;
                Ok(DirEntry::new(name, stat.file_type).with_identity(stat.dev, stat.ino))
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
/// stands at, the path a Policy judges. A Ruby object answers `stat` alone,
/// so it holds no links to follow.
impl InodeOperations for RubyFilesystem {
    fn dentry_ops(&self) -> Option<Arc<dyn DentryOperations>> {
        Some(Arc::new(*self))
    }

    /// A file open here answers for itself even once its name is gone, as 9p
    /// asks an open fid: its `size`, and its `stat` where it has one. It stays
    /// the entry its lookup recorded; anything else is asked by its path.
    fn getattr(&self, dentry: &Dentry) -> Result<Metadata, FilesystemError> {
        let Some(file) = open_file(dentry) else {
            return self.stat(dentry.path_raw());
        };
        let metadata = ask(
            file.get(),
            "size",
            FilesystemError::Unsupported,
            |_, file| {
                let metadata = Metadata::new(FileType::File, file.funcall("size", ())?);
                if file.respond_to("stat", false)? {
                    with_stat_record(file.funcall("stat", ())?, metadata)
                } else {
                    Ok(metadata)
                }
            },
        )?;
        Ok(match dentry.inode().and_then(|inode| inode.identity()) {
            Some(identity) => identity.onto(metadata),
            None => metadata,
        })
    }

    fn setattr(&self, dentry: &Dentry, attr: &Iattr) -> Option<Result<(), FilesystemError>> {
        let len = attr.size?;
        let file = open_file(dentry)?;
        Some(change(
            file.get(),
            "truncate",
            (len,),
            FilesystemError::ReadOnlyFilesystem,
        ))
    }

    /// A Ruby object matches names exactly, as its contract asks. A lookup
    /// asks its `stat` what stands at the name, as FUSE asks its daemon.
    fn lookup(&self, dentry: &Dentry) -> Option<Result<Lookup, FilesystemError>> {
        if dentry.name().len() > NAME_MAX {
            return Some(Err(FilesystemError::InvalidFilename));
        }
        Some(match self.stat(dentry.path_raw()) {
            Ok(found) => Ok(Lookup::Found(Inode::new(&found, Arc::new(*self)))),
            Err(FilesystemError::NotFound) => Ok(Lookup::Absent),
            Err(err) => Err(err),
        })
    }

    /// A directory opens without asking the object anything; it is asked
    /// for the names only when the guest lists them.
    fn open(
        &self,
        dentry: &Dentry,
        options: &OpenOptions,
    ) -> Option<Result<Box<dyn FileOperations>, FilesystemError>> {
        if options.directory {
            return Some(Ok(Box::new(RubyDir(*self))));
        }
        Some(self.open_at(dentry, options))
    }

    fn create(
        &self,
        dentry: &Dentry,
        options: &OpenOptions,
    ) -> Option<Result<Box<dyn FileOperations>, FilesystemError>> {
        Some(self.open_at(dentry, options))
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

/// A directory of the Ruby object, open: it asks the object by the path the
/// dentry it is handed stands at.
struct RubyDir(RubyFilesystem);

impl FileOperations for RubyDir {
    fn iterate_shared(&mut self, dentry: &Dentry) -> Result<Vec<DirEntry>, FilesystemError> {
        self.0.entries_at(dentry)
    }
}

/// One file the Ruby object opened, which only the guest's descriptor
/// refers to.
struct RubyFile {
    file: Arc<PinnedValue>,
    closed: bool,
}

/// The file open on a dentry. A dentry holds at most one, as `revalidate`
/// has the next walk take a dentry of its own once one is open.
#[derive(Default)]
struct OpenRubyFile(Mutex<Weak<PinnedValue>>);

/// The file open on `dentry`, while one is.
fn open_file(dentry: &Dentry) -> Option<Arc<PinnedValue>> {
    let inode = dentry.inode()?;
    let open = inode
        .private()
        .get()?
        .clone()
        .downcast::<OpenRubyFile>()
        .ok()?;
    let file = open.0.lock().unwrap_or_else(PoisonError::into_inner);
    file.upgrade()
}

/// The backend changes outside the runtime, so a dentry someone holds is
/// asked again, as FUSE compares what a fresh lookup finds, and one no longer
/// naming what it recorded gets a new one. An open file answers for itself,
/// so a walk finding a dentry one is open on takes one of its own.
impl DentryOperations for RubyFilesystem {
    fn revalidate(
        &self,
        _dir: &dyn InodeOperations,
        dentry: &Dentry,
    ) -> Result<bool, FilesystemError> {
        let Some(inode) = dentry.inode().filter(|_| open_file(dentry).is_none()) else {
            return Ok(dentry.inode().is_none());
        };
        match self.stat(dentry.path_raw()) {
            Ok(found) => Ok(Identity::of(&found) == inode.identity()),
            Err(FilesystemError::NotFound | FilesystemError::NotADirectory) => Ok(false),
            Err(err) => Err(err),
        }
    }
}

impl RubyFile {
    fn pin(ruby: &Ruby, file: Value) -> Self {
        Self {
            file: Arc::new(PinnedValue::new(ruby, file)),
            closed: false,
        }
    }

    /// The file's `append` writes all of `buf` at its end in one step and
    /// answers where the file then ends, as `IO#pos` does after a write to
    /// a file opened to append. A file lacking it cannot append, since its
    /// `size` and `pwrite` are two steps another writer can land between.
    fn append(&mut self, iocb: &mut Kiocb, buf: &[u8]) -> Result<usize, FilesystemError> {
        let end: u64 = ask(
            self.file.get(),
            "append",
            FilesystemError::Unsupported,
            |ruby, file| file.funcall("append", (ruby.str_from_slice(buf),)),
        )?;
        if end < buf.len() as u64 {
            return Err(FilesystemError::Other(format!(
                "append answered the file ending at {end}, before the {} bytes it wrote",
                buf.len()
            )));
        }
        iocb.pos = end;
        Ok(buf.len())
    }

    /// Ask the file to close, a file lacking `close` having nothing to
    /// release.
    fn close(&mut self) -> Result<(), FilesystemError> {
        self.closed = true;
        change_if_answered(self.file.get(), "close")
    }
}

impl FileOperations for RubyFile {
    fn read_iter(&mut self, iocb: &mut Kiocb, buf: &mut [u8]) -> Result<usize, FilesystemError> {
        let pos = iocb.pos;
        let read = ask(
            self.file.get(),
            "pread",
            FilesystemError::Unsupported,
            |ruby, file| {
                // `IO#pread` raises `EOFError` at the end of a file, which reads as nothing.
                let read: Option<RString> = match file.funcall("pread", (buf.len(), pos)) {
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
        )?;
        iocb.pos += read as u64;
        Ok(read)
    }

    fn write_iter(&mut self, iocb: &mut Kiocb, buf: &[u8]) -> Result<usize, FilesystemError> {
        if iocb.append {
            return self.append(iocb, buf);
        }
        let pos = iocb.pos;
        let written = ask(
            self.file.get(),
            "pwrite",
            FilesystemError::ReadOnlyFilesystem,
            |ruby, file| file.funcall("pwrite", (ruby.str_from_slice(buf), pos)),
        )
        .and_then(|written: usize| match written {
            written if written <= buf.len() => Ok(written),
            written => Err(FilesystemError::Other(format!(
                "pwrite answered {written} bytes written of the {} it was handed",
                buf.len()
            ))),
        })?;
        iocb.pos += written as u64;
        Ok(written)
    }

    /// A file lacking `fsync` has nothing of its own to flush.
    fn fsync(&mut self) -> Result<(), FilesystemError> {
        change_if_answered(self.file.get(), "fsync")
    }

    fn flush(&mut self) -> Result<(), FilesystemError> {
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
