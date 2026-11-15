//! The resource table preview2 hands the guest its handles from, as
//! wasmtime's `ResourceTable` is: it numbers its own, apart from preview1's
//! descriptors, holds every file either encoding opens and counts each
//! against the open files, and charges every entry the guest makes to the
//! Invocation's memory, as Linux's `filp` cache is accounted to the memory
//! cgroup of the task that allocates it.

use std::any::Any;
use std::collections::BTreeMap;
use std::sync::Arc;

use kobako_runtime::memcg::{MemoryBudget, MemoryCharge};
use kobako_runtime::vfs::OpenFiles;
use kobako_runtime::Errno;

/// A guest's handle to one kind of resource, as wasmtime's `Resource<T>`
/// names the host type `T` its index stands for. The bindings implement it
/// for each WIT resource, as `bindgen!` takes `with:`.
pub(crate) trait Handle: Copy {
    type Host: Any + Send;

    fn new(index: u32) -> Self;

    fn index(self) -> u32;
}

/// The most entries the resource table holds, as wasmtime's
/// `ResourceTable::max_capacity` bounds a table.
pub(crate) const MAX_TABLE_ENTRIES: usize = 1_000_000;

/// One handle's resource, and the handle it is a child of, as wasmtime's
/// `ResourceTable` keeps each entry's parent.
struct Entry {
    resource: Box<dyn Any + Send>,
    parent: Option<u32>,
    children: usize,
    /// The lends out on it, as the Canonical ABI counts a handle's lends;
    /// its drop traps while any is out.
    lends: usize,
    /// Whether it counts against the open files, as a host-side producer
    /// does and a view does not.
    counted: bool,
    /// What the entry and its resource hold, against the Invocation's
    /// memory, as each `struct file` is charged through `SLAB_ACCOUNT`.
    _memory: MemoryCharge,
}

/// The room one file takes in the table before it opens, as Linux's
/// `get_unused_fd_flags` counts it and `alloc_empty_file` charges it before
/// the walk. Dropped unused, it gives both back.
pub(crate) struct FileRoom {
    count: FileCount,
    memory: MemoryCharge,
}

/// One file counted against the open files until the table takes it over.
struct FileCount(Option<OpenFiles>);

impl Drop for FileCount {
    fn drop(&mut self) {
        if let Some(open_files) = self.0.take() {
            open_files.close();
        }
    }
}

pub(crate) struct Table {
    entries: BTreeMap<u32, Entry>,
    /// The numbers given back, the latest taken first, as wasmtime's
    /// `ResourceTable` reuses its free list.
    free: Vec<u32>,
    /// The number past every one handed out so far.
    next: u32,
    open_files: OpenFiles,
    memory: Arc<MemoryBudget>,
}

impl Table {
    /// A table holding nothing, counting its files against `open_files` and
    /// its entries against `memory`. Numbers start at 1, as the Canonical ABI
    /// keeps 0 for no handle.
    pub(crate) fn new(open_files: OpenFiles, memory: Arc<MemoryBudget>) -> Self {
        Self {
            entries: BTreeMap::new(),
            free: Vec::new(),
            next: 1,
            open_files,
            memory,
        }
    }

    /// The room a host-side producer's file takes, as a socket's, a
    /// lookup's, or an exchange's does: none past the open file limit, which
    /// each WIT answers with its own code, as Linux's `socket()` answers
    /// EMFILE; a trap past the Invocation's memory, since no WIT constructor
    /// has an error for it.
    pub(crate) fn reserve_producer<H: Handle>(&self) -> wasmtime::Result<Option<FileRoom>> {
        match self.reserve_file::<H>() {
            Ok(room) => Ok(Some(room)),
            Err(Errno::OutOfMemory) => Err(wasmtime::Error::msg(
                "the guest's resources outgrow its memory",
            )),
            Err(_) => Ok(None),
        }
    }

    /// The room a file of kind `H` takes, refused as EMFILE past the open
    /// file limit or a full table, and as ENOMEM past the Invocation's
    /// memory, both before the file opens.
    pub(crate) fn reserve_file<H: Handle>(&self) -> Result<FileRoom, Errno> {
        self.reserve(size_of::<Entry>() + size_of::<H::Host>())
    }

    /// The room a file the Driver opened before the guest runs takes: counted
    /// against the open files, and charged to none of the guest's memory, as
    /// Linux charges a `struct file` to the task that allocates it.
    pub(crate) fn reserve_inherited_file(&self) -> Result<FileRoom, Errno> {
        self.reserve(0)
    }

    fn reserve(&self, bytes: usize) -> Result<FileRoom, Errno> {
        if self.entries.len() >= MAX_TABLE_ENTRIES {
            return Err(Errno::TooManyOpenFiles);
        }
        self.open_files.open()?;
        let count = FileCount(Some(self.open_files.clone()));
        let memory = MemoryCharge::new(&self.memory, bytes)?;
        Ok(FileRoom { count, memory })
    }

    /// Hand out a handle for `resource`, a file opened into `room`.
    pub(crate) fn install<H: Handle>(&mut self, room: FileRoom, resource: H::Host) -> H {
        let FileRoom { mut count, memory } = room;
        count.0 = None;
        H::new(self.place(Box::new(resource), None, true, memory))
    }

    /// Hand out a handle for `resource`, a view that holds no file of its
    /// own; refused past `MAX_TABLE_ENTRIES`.
    pub(crate) fn push<H: Handle>(&mut self, resource: H::Host) -> wasmtime::Result<H> {
        self.insert::<H>(resource, None).map(H::new)
    }

    /// Hand out a handle for `resource` as a child of `parent`, which cannot
    /// be dropped while it lives, as the WIT has a parent's drop trap.
    pub(crate) fn push_child<H: Handle>(
        &mut self,
        resource: H::Host,
        parent: impl Handle,
    ) -> wasmtime::Result<H> {
        let parent = parent.index();
        self.entry(parent)?;
        let handle = self.insert::<H>(resource, Some(parent))?;
        self.entries
            .get_mut(&parent)
            .expect("the parent was just found")
            .children += 1;
        Ok(H::new(handle))
    }

    /// Number `resource` from the free list, else past every number handed
    /// out; refused past `MAX_TABLE_ENTRIES`, and past the Invocation's
    /// memory as a trap, since no WIT constructor has an error for it.
    fn insert<H: Handle>(
        &mut self,
        resource: H::Host,
        parent: Option<u32>,
    ) -> wasmtime::Result<u32> {
        if self.entries.len() >= MAX_TABLE_ENTRIES {
            return Err(wasmtime::Error::msg(
                "the guest holds as many resources as a table may",
            ));
        }
        let memory = MemoryCharge::new(&self.memory, size_of::<Entry>() + size_of::<H::Host>())
            .map_err(|_| wasmtime::Error::msg("the guest's resources outgrow its memory"))?;
        Ok(self.place(Box::new(resource), parent, false, memory))
    }

    /// Number `resource` from the free list, else past every number handed
    /// out.
    fn place(
        &mut self,
        resource: Box<dyn Any + Send>,
        parent: Option<u32>,
        counted: bool,
        memory: MemoryCharge,
    ) -> u32 {
        let handle = self.free.pop().unwrap_or_else(|| {
            self.next += 1;
            self.next - 1
        });
        let entry = Entry {
            resource,
            parent,
            children: 0,
            lends: 0,
            counted,
            _memory: memory,
        };
        self.entries.insert(handle, entry);
        handle
    }

    /// Whether `parent` still has a child of kind `H`, as wasmtime's
    /// `iter_children` answers.
    pub(crate) fn has_child<H: Handle>(&self, parent: impl Handle) -> bool {
        let parent = Some(parent.index());
        self.entries
            .values()
            .any(|entry| entry.parent == parent && entry.resource.is::<H::Host>())
    }

    fn entry(&self, handle: u32) -> wasmtime::Result<&Entry> {
        self.entries
            .get(&handle)
            .ok_or_else(|| wasmtime::Error::msg("the guest named a handle it does not hold"))
    }

    /// The resource `handle` stands for. A handle of another kind traps, as
    /// the Canonical ABI traps on a handle of the wrong type.
    pub(crate) fn get<H: Handle>(&self, handle: H) -> wasmtime::Result<&H::Host> {
        self.entry(handle.index())?
            .resource
            .downcast_ref()
            .ok_or_else(|| wasmtime::Error::msg("the guest named a handle of another kind"))
    }

    pub(crate) fn get_mut<H: Handle>(&mut self, handle: H) -> wasmtime::Result<&mut H::Host> {
        self.entries
            .get_mut(&handle.index())
            .ok_or_else(|| wasmtime::Error::msg("the guest named a handle it does not hold"))?
            .resource
            .downcast_mut()
            .ok_or_else(|| wasmtime::Error::msg("the guest named a handle of another kind"))
    }

    /// Lend `handle` out, so the guest cannot drop it until the lend ends,
    /// as the Canonical ABI lends a handle.
    pub(crate) fn lend(&mut self, handle: impl Handle) {
        self.lent(handle).lends += 1;
    }

    /// End one lend out on `handle`.
    pub(crate) fn end_lend(&mut self, handle: impl Handle) {
        self.lent(handle).lends -= 1;
    }

    fn lent(&mut self, handle: impl Handle) -> &mut Entry {
        self.entries
            .get_mut(&handle.index())
            .expect("a handle lent out stays in the table")
    }

    /// Take `handle` out of the table. A handle of another kind, one whose
    /// children still live, or one lent out traps, as the Canonical ABI traps
    /// on a handle of the wrong type or with lends out, and the WIT on a
    /// parent dropped first.
    pub(crate) fn delete<H: Handle>(&mut self, handle: H) -> wasmtime::Result<H::Host> {
        let index = handle.index();
        let entry = self.entry(index)?;
        if !entry.resource.is::<H::Host>() {
            return Err(wasmtime::Error::msg(
                "the guest named a handle of another kind",
            ));
        }
        if entry.children > 0 {
            return Err(wasmtime::Error::msg(
                "the guest dropped or gave up a resource while a child of it lives",
            ));
        }
        if entry.lends > 0 {
            return Err(wasmtime::Error::msg(
                "the guest dropped or gave up a resource while it is lent out",
            ));
        }
        let entry = self
            .entries
            .remove(&index)
            .expect("the entry was just found");
        self.free.push(index);
        if entry.counted {
            self.open_files.close();
        }
        if let Some(parent) = entry
            .parent
            .and_then(|parent| self.entries.get_mut(&parent))
        {
            parent.children -= 1;
        }
        Ok(*entry
            .resource
            .downcast()
            .expect("the entry was just found to be of this kind"))
    }
}
