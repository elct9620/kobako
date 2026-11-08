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

/// `file`, for views to share.
pub(crate) fn share(file: File) -> SharedFile {
    Arc::new(Mutex::new(file))
}

/// The shared `file`, which a read or a write takes `&mut`.
pub(crate) fn lock(file: &SharedFile) -> MutexGuard<'_, File> {
    file.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The numbers both encodings hand out, from one space, as Linux's one
/// fdtable numbers a regular file and a socket alike: a preview1 descriptor
/// and a preview2 handle never share a number, and each counts once against
/// the Invocation's open files. A socket's number holds its file, so a
/// preview1 call on it reaches the socket.
#[derive(Clone)]
pub(crate) struct Numbers {
    taken: Arc<Mutex<BTreeMap<u32, Option<SharedFile>>>>,
    open_files: OpenFiles,
}

impl Numbers {
    pub(crate) fn new(open_files: OpenFiles) -> Self {
        Self {
            taken: Arc::default(),
            open_files,
        }
    }

    fn taken(&self) -> MutexGuard<'_, BTreeMap<u32, Option<SharedFile>>> {
        self.taken.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Have `n`, a socket's number, stand for its `file`, as `fd_install`
    /// fills an fd with a `struct file`.
    pub(crate) fn install(&self, n: u32, file: SharedFile) {
        if let Some(slot) = self.taken().get_mut(&n) {
            *slot = Some(file);
        }
    }

    /// The socket file `n` stands for, if a socket's number holds one.
    pub(crate) fn socket_file(&self, n: u32) -> Option<SharedFile> {
        self.taken().get(&n).cloned().flatten()
    }

    /// The lowest number no table holds, as Linux's `alloc_fd` finds it;
    /// `None` when the Invocation holds as many as it may. Nothing is taken
    /// until `take`, as `fd_install` fills what was found.
    pub(crate) fn lowest_unused(&self) -> Option<u32> {
        if self.open_files.is_full() {
            return None;
        }
        let taken = self.taken();
        (0..=u32::MAX).find(|n| !taken.contains_key(n))
    }

    /// Take `n`, a number `lowest_unused` found or one just given back.
    pub(crate) fn take(&self, n: u32) {
        self.open_files
            .open()
            .expect("a number lowest_unused found, or one just given back, has room in the count");
        self.taken().insert(n, None);
    }

    /// Give `n` back, so the next number found may be it.
    pub(crate) fn give_back(&self, n: u32) {
        if self.taken().remove(&n).is_some() {
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
            numbers.taken().insert(*fd, None);
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
