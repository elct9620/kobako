//! The guest's descriptor table: what each preview1 file descriptor
//! stands for within one invocation.
//!
//! kobako answers every preview1 call that reads this table, so the guest
//! sees one table and no call falls through to a second one that
//! disagrees. Numbering follows Linux's `alloc_fd`: the lowest number no
//! table holds is taken, a preview2 handle's included.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use kobako_runtime::filesystem::FilesystemError;
use kobako_runtime::mount::Mount;
use kobako_runtime::net::ProtoOps;
use kobako_runtime::profile::Profile;
use kobako_runtime::security::{restrict_self, WalkGuard};
use kobako_runtime::vfs::{Cred, File, OpenFiles};

/// What one descriptor stands for, as Linux's fd table holds a `struct file`.
pub(crate) struct Descriptor {
    pub(crate) file: File,
    /// The guest path a preopen names; a Mount's own root has one.
    pub(crate) preopen_path: Option<String>,
}

impl Descriptor {
    /// The descriptor of `file`, naming no preopen.
    pub(crate) fn for_file(file: File) -> Self {
        Self {
            file,
            preopen_path: None,
        }
    }
}

/// A file every view of it shares, as handles on one `struct file` do.
pub(crate) type SharedFile = Arc<Mutex<File>>;

/// The shared `file`, which a read or a write takes `&mut`.
pub(crate) fn lock(file: &SharedFile) -> MutexGuard<'_, File> {
    file.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The most numbers the space holds, files and views alike, as wasmtime's
/// `ResourceTable::max_capacity` bounds a table.
pub(crate) const MAX_NUMBERS: usize = 1_000_000;

/// The numbers both encodings hand out, from one space, as Linux's one
/// fdtable numbers a regular file and a socket alike: a preview1 descriptor
/// and a preview2 handle never share a number. A file — a descriptor, or a
/// preview2 resource with a host-side producer behind it — counts once
/// against the Invocation's open files; a view of one counts only against
/// `MAX_NUMBERS`. A socket's number holds its file, so a preview1 call on it
/// reaches the socket.
#[derive(Clone)]
pub(crate) struct Numbers {
    space: Arc<Mutex<Space>>,
    open_files: OpenFiles,
}

#[derive(Default)]
struct Space {
    taken: BTreeMap<u32, Slot>,
    /// No number below this is free, as Linux's `next_fd` hints `alloc_fd`.
    next: u32,
}

/// A socket a number holds: its file, the protocol behind it, and its kind.
#[derive(Clone)]
struct InstalledSocket {
    file: SharedFile,
    ops: Arc<dyn ProtoOps>,
    kind: SocketType,
}

/// What one taken number holds.
struct Slot {
    socket: Option<InstalledSocket>,
    /// Whether it counts against the open files.
    is_file: bool,
}

/// Which kind of socket a number holds, as Linux's `sock->type` names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SocketType {
    Stream,
    Datagram,
}

impl Numbers {
    pub(crate) fn new(open_files: OpenFiles) -> Self {
        Self {
            space: Arc::default(),
            open_files,
        }
    }

    fn space(&self) -> MutexGuard<'_, Space> {
        self.space.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Have `n`, a socket's number, stand for its `file` and the protocol
    /// behind it, as `fd_install` fills an fd with a `struct file` whose
    /// `struct socket` carries its `proto_ops`.
    pub(crate) fn install(
        &self,
        n: u32,
        file: SharedFile,
        ops: Arc<dyn ProtoOps>,
        kind: SocketType,
    ) {
        if let Some(slot) = self.space().taken.get_mut(&n) {
            slot.socket = Some(InstalledSocket { file, ops, kind });
        }
    }

    /// The socket file `n` stands for, if a socket's number holds one.
    pub(crate) fn socket_file(&self, n: u32) -> Option<SharedFile> {
        self.installed(n).map(|socket| socket.file)
    }

    /// The socket `n` holds and its kind, if a socket's number holds one.
    pub(crate) fn socket_of(&self, n: u32) -> Option<(SharedFile, SocketType)> {
        self.installed(n).map(|socket| (socket.file, socket.kind))
    }

    /// The protocol's operations the socket `n` holds answers through, as
    /// `sock_from_file` reaches a file's socket.
    pub(crate) fn proto_ops(&self, n: u32) -> Option<Arc<dyn ProtoOps>> {
        self.installed(n).map(|socket| socket.ops)
    }

    fn installed(&self, n: u32) -> Option<InstalledSocket> {
        self.space()
            .taken
            .get(&n)
            .and_then(|slot| slot.socket.clone())
    }

    /// The lowest number free for a file, as Linux's `alloc_fd` finds it;
    /// `None` when the Invocation holds as many files as it may. Nothing is
    /// taken until `take`, as `fd_install` fills what was found.
    pub(crate) fn lowest_unused(&self) -> Option<u32> {
        match self.open_files.is_full() {
            true => None,
            false => self.lowest_free(),
        }
    }

    /// The lowest number free for a view; `None` past `MAX_NUMBERS`.
    pub(crate) fn lowest_free(&self) -> Option<u32> {
        let space = self.space();
        if space.taken.len() >= MAX_NUMBERS {
            return None;
        }
        (space.next..=u32::MAX).find(|n| !space.taken.contains_key(n))
    }

    /// Take `n` for a file, a number `lowest_unused` found or one just given
    /// back.
    pub(crate) fn take(&self, n: u32) {
        self.open_files
            .open()
            .expect("a number lowest_unused found, or one just given back, has room in the count");
        self.occupy(n, true);
    }

    /// Take `n` for a view, a number `lowest_free` found.
    pub(crate) fn take_view(&self, n: u32) {
        self.occupy(n, false);
    }

    fn occupy(&self, n: u32, is_file: bool) {
        let mut space = self.space();
        space.taken.insert(
            n,
            Slot {
                socket: None,
                is_file,
            },
        );
        if n == space.next {
            space.next = (n..=u32::MAX)
                .find(|n| !space.taken.contains_key(n))
                .unwrap_or(u32::MAX);
        }
    }

    /// Give `n` back, so the next number found may be it.
    pub(crate) fn give_back(&self, n: u32) {
        let mut space = self.space();
        let Some(slot) = space.taken.remove(&n) else {
            return;
        };
        space.next = space.next.min(n);
        drop(space);
        if slot.is_file {
            self.open_files.close();
        }
    }
}

pub(crate) struct Descriptors {
    used: BTreeMap<u32, Descriptor>,
    /// The numbers this table shares with the preview2 resources.
    numbers: Numbers,
    /// Who the guest's calls are made for, as a task holds its `cred`.
    cred: Arc<Cred>,
    /// What a descriptor's call asks before it starts.
    walk_guard: WalkGuard,
    /// The isolation the guest runs under, which decides what it sees of
    /// the times a Filesystem reports.
    profile: Profile,
}

impl Descriptors {
    /// The table every invocation starts from: stdin, then the two
    /// Channels, then one preopen per Mount from descriptor 3. Each counts
    /// against `open_files`, those included, as a process's `RLIMIT_NOFILE`
    /// counts every one it holds. Each Mount's root opens as `opener`, the
    /// Cred the Driver prepared, before its Policy applies.
    pub(crate) fn new(
        stdin: File,
        stdout: File,
        stderr: File,
        mounts: Vec<Mount>,
        numbers: Numbers,
        opener: Arc<Cred>,
        profile: Profile,
    ) -> Result<Self, FilesystemError> {
        let mut used = BTreeMap::from([
            (0, Descriptor::for_file(stdin)),
            (1, Descriptor::for_file(stdout)),
            (2, Descriptor::for_file(stderr)),
        ]);
        // Each Mount's root opens before its Policy applies, as a Linux task
        // opens what it keeps and then calls `landlock_restrict_self`, so a
        // Policy judges only what the guest asks for.
        for (fd, mount) in (3..).zip(&mounts) {
            used.insert(
                fd,
                Descriptor {
                    file: File::root(mount.mnt(), &opener)?,
                    preopen_path: Some(mount.at().to_string()),
                },
            );
        }
        for fd in used.keys() {
            numbers.open_files.open()?;
            numbers.occupy(*fd, true);
        }
        let cred = restrict_self(&opener, &mounts);
        let walk_guard = WalkGuard::new(&mounts);
        Ok(Self {
            used,
            numbers,
            cred,
            walk_guard,
            profile,
        })
    }

    /// Who the guest's calls are made for.
    pub(crate) fn cred(&self) -> &Arc<Cred> {
        &self.cred
    }

    /// What every call through a descriptor asks first, so a Policy judges the
    /// path it stands at.
    pub(crate) fn walk_guard(&self) -> &WalkGuard {
        &self.walk_guard
    }

    /// The isolation the guest runs under.
    pub(crate) fn profile(&self) -> Profile {
        self.profile
    }

    pub(crate) fn get(&self, fd: u32) -> Option<&Descriptor> {
        self.used.get(&fd)
    }

    pub(crate) fn get_mut(&mut self, fd: u32) -> Option<&mut Descriptor> {
        self.used.get_mut(&fd)
    }

    /// The socket file a preview2 socket installed at `fd`.
    pub(crate) fn socket(&self, fd: u32) -> Option<SharedFile> {
        self.numbers.socket_file(fd)
    }

    /// The socket a preview2 socket installed at `fd`, and its kind.
    pub(crate) fn socket_of(&self, fd: u32) -> Option<(SharedFile, SocketType)> {
        self.numbers.socket_of(fd)
    }

    /// The protocol's operations of the socket a preview2 socket installed at
    /// `fd`.
    pub(crate) fn proto_ops(&self, fd: u32) -> Option<Arc<dyn ProtoOps>> {
        self.numbers.proto_ops(fd)
    }

    /// Take `fd` out of the table, freeing its number for reuse.
    pub(crate) fn remove(&mut self, fd: u32) -> Option<Descriptor> {
        let descriptor = self.used.remove(&fd)?;
        self.numbers.give_back(fd);
        Some(descriptor)
    }

    /// Place `descriptor` at `fd`, a number the caller has just freed or
    /// `lowest_unused` found, as Linux's `fd_install` does.
    pub(crate) fn place(&mut self, fd: u32, descriptor: Descriptor) {
        self.numbers.take(fd);
        self.used.insert(fd, descriptor);
    }

    /// The number the next descriptor takes, the lowest one no table holds,
    /// as Linux's `get_unused_fd_flags` finds it; `None` when the Invocation
    /// holds as many as it may.
    pub(crate) fn lowest_unused(&self) -> Option<u32> {
        self.numbers.lowest_unused()
    }

    /// Add `descriptor` under the number `lowest_unused` finds; `None` when
    /// the table is full.
    #[cfg(test)]
    pub(crate) fn push(&mut self, descriptor: Descriptor) -> Option<u32> {
        let fd = self.lowest_unused()?;
        self.place(fd, descriptor);
        Some(fd)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kobako_runtime::filesystem::{create_pipe_files, PipeInodeInfo};
    use kobako_runtime::security::prepare_creds_within;

    fn pipe_end() -> File {
        create_pipe_files(&PipeInodeInfo::new(1), &prepare_creds_within(&[], None)).1
    }

    // @behavior RT-189
    #[test]
    fn a_new_descriptor_takes_the_lowest_closed_number() {
        let mut table = Descriptors::new(
            pipe_end(),
            pipe_end(),
            pipe_end(),
            Vec::new(),
            Numbers::new(OpenFiles::new(None)),
            prepare_creds_within(&[], None),
            Profile::Hermetic,
        )
        .unwrap_or_else(|_| panic!("a table holding no Mount must make"));
        let mut open = || {
            table
                .push(Descriptor::for_file(pipe_end()))
                .expect("a table with no limit takes every descriptor")
        };
        let numbers = [open(), open(), open()];
        assert_eq!(numbers, [3, 4, 5], "descriptors must number from 3");
        table.remove(3);
        table.remove(4);
        assert_eq!(
            (
                table.push(Descriptor::for_file(pipe_end())),
                table.push(Descriptor::for_file(pipe_end())),
                table.push(Descriptor::for_file(pipe_end())),
            ),
            (Some(3), Some(4), Some(6)),
            "path_open's descriptors must take the lowest closed number first, as Linux's alloc_fd \
             does, and then the one past the highest"
        );
    }
}
