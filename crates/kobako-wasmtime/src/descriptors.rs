//! The guest's descriptor table: what each preview1 file descriptor
//! stands for within one invocation.
//!
//! kobako answers every preview1 call that reads this table, so the guest
//! sees one table and no call falls through to a second one that
//! disagrees. Numbering follows Linux's `alloc_fd`: the lowest closed number
//! is reused before a fresh one is taken.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use kobako_runtime::filesystem::FilesystemError;
use kobako_runtime::mount::Mount;
use kobako_runtime::profile::Profile;
use kobako_runtime::security::prepare_creds;
use kobako_runtime::vfs::{Cred, File};

/// What one descriptor stands for, as Linux's fd table holds a `struct file`.
pub(crate) struct Descriptor {
    pub(crate) file: File,
    /// The guest path a preopen names; a Mount's own root has one.
    pub(crate) preopen: Option<String>,
}

impl Descriptor {
    /// The descriptor of `file`, naming no preopen.
    pub(crate) fn of(file: File) -> Self {
        Self {
            file,
            preopen: None,
        }
    }
}

pub(crate) struct Descriptors {
    used: BTreeMap<u32, Descriptor>,
    free: BTreeSet<u32>,
    limit: Option<usize>,
    /// Who the guest's calls are made for, as a task holds its `cred`.
    cred: Arc<Cred>,
    /// The isolation the guest runs under, which decides what it sees of
    /// the times a Filesystem reports.
    profile: Profile,
}

impl Descriptors {
    /// The table every invocation starts from: stdin, then the two
    /// Channels, then one preopen per Mount from descriptor 3. It holds at
    /// most `limit` descriptors, those included, as a process's
    /// `RLIMIT_NOFILE` counts every one it holds.
    pub(crate) fn new(
        stdin: File,
        stdout: File,
        stderr: File,
        mounts: Vec<Mount>,
        limit: Option<usize>,
        profile: Profile,
    ) -> Result<Self, FilesystemError> {
        let mut used = BTreeMap::from([
            (0, Descriptor::of(stdin)),
            (1, Descriptor::of(stdout)),
            (2, Descriptor::of(stderr)),
        ]);
        let cred = prepare_creds(&mounts);
        for (fd, mount) in (3..).zip(&mounts) {
            used.insert(
                fd,
                Descriptor {
                    file: File::root(mount.mnt(), cred.clone())?,
                    preopen: Some(mount.at().to_string()),
                },
            );
        }
        Ok(Self {
            used,
            free: BTreeSet::new(),
            limit,
            cred,
            profile,
        })
    }

    /// Who the guest's calls are made for.
    pub(crate) fn cred(&self) -> &Arc<Cred> {
        &self.cred
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

    /// Take `fd` out of the table, freeing its number for reuse.
    pub(crate) fn remove(&mut self, fd: u32) -> Option<Descriptor> {
        let descriptor = self.used.remove(&fd)?;
        self.free.insert(fd);
        Some(descriptor)
    }

    /// Place `descriptor` at `fd`, a number the caller has just freed.
    pub(crate) fn place(&mut self, fd: u32, descriptor: Descriptor) {
        self.free.remove(&fd);
        self.used.insert(fd, descriptor);
    }

    /// Whether the table holds as many descriptors as it may.
    pub(crate) fn is_full(&self) -> bool {
        self.limit.is_some_and(|limit| self.used.len() >= limit)
    }

    /// Add `descriptor` under the lowest freed number, or the one past
    /// the highest in use; `None` when the table is full.
    pub(crate) fn push(&mut self, descriptor: Descriptor) -> Option<u32> {
        if self.is_full() {
            return None;
        }
        let fd = match self.free.pop_first() {
            Some(fd) => fd,
            None => match self.used.last_key_value() {
                Some((last, _)) => last.checked_add(1)?,
                None => 0,
            },
        };
        self.used.insert(fd, descriptor);
        Some(fd)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kobako_runtime::vfs::{create_pipe_files, PipeInodeInfo};

    fn pipe_end() -> File {
        create_pipe_files(&PipeInodeInfo::new(1)).1
    }

    // @behavior RT-189
    #[test]
    fn a_new_descriptor_takes_the_lowest_closed_number() {
        let mut table = Descriptors::new(
            pipe_end(),
            pipe_end(),
            pipe_end(),
            Vec::new(),
            None,
            Profile::Hermetic,
        )
        .unwrap_or_else(|_| panic!("a table holding no Mount must make"));
        let mut open = || {
            table
                .push(Descriptor::of(pipe_end()))
                .expect("a table with no limit takes every descriptor")
        };
        let numbers = [open(), open(), open()];
        assert_eq!(numbers, [3, 4, 5], "descriptors must number from 3");
        table.remove(3);
        table.remove(4);
        assert_eq!(
            (
                table.push(Descriptor::of(pipe_end())),
                table.push(Descriptor::of(pipe_end())),
                table.push(Descriptor::of(pipe_end())),
            ),
            (Some(3), Some(4), Some(6)),
            "path_open's descriptors must take the lowest closed number first, as Linux's alloc_fd \
             does, and then the one past the highest"
        );
    }
}
