//! The guest's descriptor table: what each preview1 file descriptor
//! stands for within one invocation.
//!
//! kobako answers every preview1 call that reads this table, so the guest
//! sees one table and no call falls through to a second one that
//! disagrees. Numbering follows Linux's `alloc_fd`: the lowest number the
//! table does not hold is taken. preview2's resource table numbers its own,
//! as wasmtime-wasi's preview1 numbers apart from its `ResourceTable`, and
//! both count against one `OpenFiles`.

use std::collections::BTreeMap;
use std::sync::Arc;

use kobako_runtime::cred::Cred;
use kobako_runtime::grant::Grant;
use kobako_runtime::security::{restrict_self, WalkGuard};
use kobako_runtime::vfs::{File, OpenFiles};
use kobako_runtime::Errno;

use crate::wasi::MAX_TABLE_ENTRIES;

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

pub(crate) struct Descriptors {
    used: BTreeMap<u32, Descriptor>,
    /// No number below this is free, as Linux's `next_fd` hints `alloc_fd`.
    next_fd: u32,
    /// The files the Invocation holds, a preview2 resource's included.
    open_files: OpenFiles,
    /// Who the guest's calls are made for, as a task holds its `cred`.
    cred: Arc<Cred>,
    /// What a descriptor's call asks before it starts.
    walk_guard: WalkGuard,
}

impl Descriptors {
    /// The table every invocation starts from: stdin, then the two
    /// Channels, then one preopen per Mount `grants` holds, from descriptor
    /// 3. Each counts against `open_files`, those included, as a process's
    /// `RLIMIT_NOFILE` counts every one it holds. Each Mount's root opens as
    /// `opener`, the Cred the Driver prepared, before the Grants' policies
    /// apply.
    pub(crate) fn new(
        stdin: File,
        stdout: File,
        stderr: File,
        grants: &[Grant],
        open_files: OpenFiles,
        opener: Arc<Cred>,
    ) -> Result<Self, Errno> {
        let mut used = BTreeMap::from([
            (0, Descriptor::for_file(stdin)),
            (1, Descriptor::for_file(stdout)),
            (2, Descriptor::for_file(stderr)),
        ]);
        // Each Mount's root opens before its Policy applies, as a Linux task
        // opens what it keeps and then calls `landlock_restrict_self`, so a
        // Policy judges only what the guest asks for.
        let mounts = grants
            .iter()
            .filter_map(|grant| match grant {
                Grant::Mount(mount) => Some(mount),
                _ => None,
            })
            .collect::<Vec<_>>();
        for (fd, mount) in (3..).zip(&mounts) {
            used.insert(
                fd,
                Descriptor {
                    file: File::root(mount.mnt(), &opener)?,
                    preopen_path: Some(mount.at().to_string()),
                },
            );
        }
        for _ in used.keys() {
            open_files.open()?;
        }
        let cred = restrict_self(&opener, grants);
        let walk_guard = WalkGuard::new(mounts);
        Ok(Self {
            next_fd: used.len() as u32,
            used,
            open_files,
            cred,
            walk_guard,
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

    pub(crate) fn get(&self, fd: u32) -> Option<&Descriptor> {
        self.used.get(&fd)
    }

    pub(crate) fn get_mut(&mut self, fd: u32) -> Option<&mut Descriptor> {
        self.used.get_mut(&fd)
    }

    /// Take `fd` out of the table, freeing its number for reuse.
    pub(crate) fn remove(&mut self, fd: u32) -> Option<Descriptor> {
        let descriptor = self.used.remove(&fd)?;
        self.open_files.close();
        self.next_fd = self.next_fd.min(fd);
        Some(descriptor)
    }

    /// Place `descriptor` at `fd`, a number the caller has just freed or
    /// `lowest_unused` found, as Linux's `fd_install` does.
    pub(crate) fn place(&mut self, fd: u32, descriptor: Descriptor) {
        self.open_files
            .open()
            .expect("a number lowest_unused found, or one just given back, has room in the count");
        self.used.insert(fd, descriptor);
        if fd == self.next_fd {
            self.next_fd = (fd..=u32::MAX)
                .find(|fd| !self.used.contains_key(fd))
                .unwrap_or(u32::MAX);
        }
    }

    /// The number the next descriptor takes, the lowest one the table does
    /// not hold, as Linux's `get_unused_fd_flags` finds it; `None` when the
    /// Invocation holds as many files as it may, or the table as many as
    /// `MAX_TABLE_ENTRIES`.
    pub(crate) fn lowest_unused(&self) -> Option<u32> {
        if self.open_files.is_full() || self.used.len() >= MAX_TABLE_ENTRIES {
            return None;
        }
        (self.next_fd..=u32::MAX).find(|fd| !self.used.contains_key(fd))
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
            &[],
            OpenFiles::new(None),
            prepare_creds_within(&[], None),
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
