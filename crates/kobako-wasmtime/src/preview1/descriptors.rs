//! The guest's descriptor table: what each preview1 file descriptor
//! stands for within one invocation.
//!
//! kobako answers every preview1 call that reads this table, so the guest
//! sees one table and no call falls through to a second one that
//! disagrees. Numbering follows Linux's `alloc_fd`: the lowest number the
//! table does not hold is taken. Each number stands over a handle in
//! preview2's resource table, which holds the file and counts it, as
//! wasmtime-wasi's preview1 numbers apart from the `ResourceTable` it shares
//! with preview2. The standard streams stand over preview2 streams, as
//! `wasi:cli` hands them out, each over its end of a pipe and lent to its
//! descriptor, so the guest drops none while the descriptor stands.

use std::collections::BTreeMap;
use std::ops::{Deref, DerefMut};
use std::sync::{Arc, MutexGuard};

use kobako_runtime::cred::Cred;
use kobako_runtime::mount::Mount;
use kobako_runtime::vfs::File;
use kobako_runtime::Errno;

pub(crate) use crate::preview2::bindings::wasi_filesystem_types_0_2_12::Descriptor as FileHandle;
use crate::preview2::bindings::wasi_io_streams_0_2_12::{
    InputStream as InputHandle, OutputStream as OutputHandle,
};
use crate::preview2::table::{Handle, InputStream, OutputStream, Stream};
use crate::preview2::Table;

/// The preview2 resource one descriptor stands over, by its handle.
#[derive(Clone, Copy)]
pub(crate) enum Resource {
    File(FileHandle),
    /// stdin, read as `wasi:cli`'s `get-stdin` hands it.
    Input(InputHandle),
    /// stdout or stderr, written as `wasi:cli`'s `get-stdout` hands it.
    Output(OutputHandle),
}

impl Resource {
    /// The stream a standard stream's descriptor stands over, if it is one.
    fn stream(self, table: &Table) -> Option<&Stream> {
        match self {
            Resource::File(_) => None,
            Resource::Input(handle) => table.get(handle).ok().map(|input| &input.stream),
            Resource::Output(handle) => table.get(handle).ok().map(|output| &output.stream),
        }
    }
}

/// What one descriptor stands for, as Linux's fd table holds a `struct file`.
pub(crate) struct Descriptor {
    pub(crate) resource: Resource,
    /// The guest path a preopen names; a Mount's own root has one.
    pub(crate) preopen_path: Option<String>,
}

impl Descriptor {
    /// The descriptor of the file under `file`, naming no preopen.
    pub(crate) fn for_file(file: FileHandle) -> Self {
        Self {
            resource: Resource::File(file),
            preopen_path: None,
        }
    }

    fn for_stream(resource: Resource) -> Self {
        Self {
            resource,
            preopen_path: None,
        }
    }
}

pub(crate) struct Descriptors {
    used: BTreeMap<u32, Descriptor>,
    /// No number below this is free, as Linux's `next_fd` hints `alloc_fd`.
    next_fd: u32,
}

impl Descriptors {
    /// The table every invocation starts from: stdin, then the two
    /// Channels, then one preopen per Mount among `mounts`, from descriptor
    /// 3. Each counts against the open files in `table`, as a process's
    /// `RLIMIT_NOFILE` counts every one it holds. Each Mount's root opens as
    /// `opener`, the Cred the Driver prepared.
    pub(crate) fn new(
        stdin: File,
        stdout: File,
        stderr: File,
        mounts: &[&Mount],
        table: &mut Table,
        opener: &Arc<Cred>,
    ) -> Result<Self, Errno> {
        let mut used = BTreeMap::new();
        let input = InputStream::new(Stream::from(stdin));
        let input = lend_inherited(table, input)?;
        used.insert(0, Descriptor::for_stream(Resource::Input(input)));
        for (fd, file) in [(1, stdout), (2, stderr)] {
            let output = lend_inherited(table, OutputStream::new(Stream::from(file)))?;
            used.insert(fd, Descriptor::for_stream(Resource::Output(output)));
        }
        for (fd, mount) in (3..).zip(mounts) {
            let root = File::root(mount.mnt(), opener)?;
            used.insert(
                fd,
                Descriptor {
                    resource: Resource::File(install_inherited(table, root)?),
                    preopen_path: Some(mount.at().to_string()),
                },
            );
        }
        Ok(Self {
            next_fd: used.len() as u32,
            used,
        })
    }

    pub(crate) fn get(&self, fd: u32) -> Option<&Descriptor> {
        self.used.get(&fd)
    }

    /// Take `fd` out of the table, freeing its number for reuse.
    pub(crate) fn remove(&mut self, fd: u32) -> Option<Descriptor> {
        let descriptor = self.used.remove(&fd)?;
        self.next_fd = self.next_fd.min(fd);
        Some(descriptor)
    }

    /// Place `descriptor` at `fd`, a number the caller has just freed or
    /// `lowest_unused` found, as Linux's `fd_install` does.
    pub(crate) fn place(&mut self, fd: u32, descriptor: Descriptor) {
        self.used.insert(fd, descriptor);
        if fd == self.next_fd {
            self.next_fd = (fd..=u32::MAX)
                .find(|fd| !self.used.contains_key(fd))
                .unwrap_or(u32::MAX);
        }
    }

    /// The number the next descriptor takes, the lowest one the table does
    /// not hold, as Linux's `get_unused_fd_flags` finds it.
    pub(crate) fn lowest_unused(&self) -> Option<u32> {
        (self.next_fd..=u32::MAX).find(|fd| !self.used.contains_key(fd))
    }

    /// The file `fd` stands for, one that stands only for its path included:
    /// the one `table` holds, or the pipe's end a standard stream stands
    /// over, which answers each call as Linux's pipe does.
    pub(crate) fn file<'a>(&self, fd: u32, table: &'a Table) -> Option<FileRef<'a>> {
        let resource = self.get(fd)?.resource;
        Some(match resource {
            Resource::File(handle) => FileRef::Held(table.get(handle).ok()?),
            stream => FileRef::Stream(stream.stream(table)?.lock()),
        })
    }

    /// The file `fd` stands for, to change.
    pub(crate) fn file_mut<'a>(&self, fd: u32, table: &'a mut Table) -> Option<FileRefMut<'a>> {
        let resource = self.get(fd)?.resource;
        Some(match resource {
            Resource::File(handle) => FileRefMut::Held(table.get_mut(handle).ok()?),
            stream => FileRefMut::Stream(stream.stream(table)?.lock()),
        })
    }

    /// Take `fd` out of the table, and what it stood over out of `table`:
    /// the file to close, or none for a standard stream, whose lend ends
    /// while preview2's handle keeps the stream.
    pub(crate) fn take(&mut self, fd: u32, table: &mut Table) -> Option<Option<File>> {
        Some(match self.remove(fd)?.resource {
            Resource::File(handle) => table.delete(handle).ok(),
            Resource::Input(handle) => {
                table.end_lend(handle);
                None
            }
            Resource::Output(handle) => {
                table.end_lend(handle);
                None
            }
        })
    }

    /// Open `file` under the lowest free descriptor; `None` when the
    /// Invocation holds as many files as it may.
    #[cfg(test)]
    pub(crate) fn push_file(&mut self, file: File, table: &mut Table) -> Option<u32> {
        let room = table.reserve_file::<FileHandle>().ok()?;
        let fd = self.lowest_unused()?;
        let handle = table.install(room, file);
        self.place(fd, Descriptor::for_file(handle));
        Some(fd)
    }
}

/// The handle `resource`, opened before the guest runs, takes in `table`;
/// refused past the open file limit.
fn install_inherited<H: Handle>(table: &mut Table, resource: H::Host) -> Result<H, Errno> {
    let room = table.reserve_inherited_file()?;
    Ok(table.install(room, resource))
}

/// The handle a standard stream takes in `table`, lent to the descriptor
/// standing over it.
fn lend_inherited<H: Handle>(table: &mut Table, stream: H::Host) -> Result<H, Errno> {
    let handle = install_inherited(table, stream)?;
    table.lend(handle);
    Ok(handle)
}

/// A file a descriptor stands for, as `Descriptors::file` finds it.
pub(crate) enum FileRef<'a> {
    Held(&'a File),
    /// The pipe's end behind a standard stream, which its stream shares.
    Stream(MutexGuard<'a, File>),
}

impl Deref for FileRef<'_> {
    type Target = File;

    fn deref(&self) -> &File {
        match self {
            FileRef::Held(file) => file,
            FileRef::Stream(file) => file,
        }
    }
}

/// A file a descriptor stands for, to change, as `Descriptors::file_mut`
/// finds it.
pub(crate) enum FileRefMut<'a> {
    Held(&'a mut File),
    Stream(MutexGuard<'a, File>),
}

impl Deref for FileRefMut<'_> {
    type Target = File;

    fn deref(&self) -> &File {
        match self {
            FileRefMut::Held(file) => file,
            FileRefMut::Stream(file) => file,
        }
    }
}

impl DerefMut for FileRefMut<'_> {
    fn deref_mut(&mut self) -> &mut File {
        match self {
            FileRefMut::Held(file) => file,
            FileRefMut::Stream(file) => file,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kobako_runtime::filesystem::{create_pipe_files, PipeInodeInfo};
    use kobako_runtime::memcg::MemoryBudget;
    use kobako_runtime::security::prepare_creds_within;
    use kobako_runtime::vfs::OpenFiles;

    fn pipe_end() -> File {
        create_pipe_files(&PipeInodeInfo::new(1), &prepare_creds_within(&[], None)).1
    }

    // @behavior RT-189
    #[test]
    fn a_new_descriptor_takes_the_lowest_closed_number() {
        let mut table = Table::new(OpenFiles::new(None), MemoryBudget::new(None));
        let mut descriptors = Descriptors::new(
            pipe_end(),
            pipe_end(),
            pipe_end(),
            &[],
            &mut table,
            &prepare_creds_within(&[], None),
        )
        .unwrap_or_else(|_| panic!("a table holding no Mount must make"));
        let mut open = |descriptors: &mut Descriptors| {
            let fd = descriptors
                .lowest_unused()
                .expect("a table with no limit numbers every descriptor");
            let file =
                install_inherited(&mut table, pipe_end()).expect("a table with no limit takes it");
            descriptors.place(fd, Descriptor::for_file(file));
            fd
        };
        let numbers = [
            open(&mut descriptors),
            open(&mut descriptors),
            open(&mut descriptors),
        ];
        assert_eq!(numbers, [3, 4, 5], "descriptors must number from 3");
        descriptors.remove(3);
        descriptors.remove(4);
        assert_eq!(
            (
                open(&mut descriptors),
                open(&mut descriptors),
                open(&mut descriptors),
            ),
            (3, 4, 6),
            "path_open's descriptors must take the lowest closed number first, as Linux's alloc_fd \
             does, and then the one past the highest"
        );
    }
}
