//! kobako's own answers to the 36 preview1 imports that read the
//! descriptor table: `fd_*`, `path_*`, `sock_*`, and `poll_oneoff`.
//!
//! wasmtime-wasi keeps its table private, so a descriptor kobako places
//! there would stay invisible to its calls. Answering every call that reads
//! a descriptor keeps the guest on one table, `crate::descriptors`, where
//! each Mount is a preopen. The other ten imports — arguments,
//! environment, clocks, process, scheduling, and randomness — stay
//! wasmtime-wasi's, so `witx/` declares only these 36. wiggle generates their
//! glue as it generates wasmtime-wasi's, so a value outside what the witx
//! defines traps there before a call starts.

mod errno;
mod fd;
mod memory;
mod path;
mod poll;
mod sock;

#[cfg(test)]
mod differential;
#[cfg(test)]
mod mount_view;
#[cfg(test)]
mod testsuite;

use wasmtime::Linker;
use wiggle::GuestPtr;

use crate::invocation::Invocation;
use errno::Answer;
use memory::GuestMemory;

// wiggle gives each generated call one parameter per witx argument, and no
// generated signature can be shrunk to fit the lint.
#[allow(clippy::too_many_arguments)]
mod witx {
    wiggle::from_witx!({
        witx: ["witx/wasi_snapshot_preview1.witx"],
        tracing: false,
        errors: { errno => trappable Error },
    });

    pub(super) mod glue {
        wiggle::wasmtime_integration!({
            witx: ["witx/wasi_snapshot_preview1.witx"],
            target: super,
            tracing: false,
            errors: { errno => trappable Error },
        });
    }
}

use witx::{types, wasi_snapshot_preview1};

impl wiggle::GuestErrorType for types::Errno {
    fn success() -> Self {
        Self::Success
    }
}

/// Define kobako's answers over wasmtime-wasi's for the same names.
pub(crate) fn shadow(linker: &mut Linker<Invocation>) -> wasmtime::Result<()> {
    linker.allow_shadowing(true);
    witx::glue::add_wasi_snapshot_preview1_to_linker(linker, |invocation: &mut Invocation| {
        invocation
    })?;
    linker.allow_shadowing(false);
    Ok(())
}

impl Invocation {
    /// Run one call against the calling instance's memory, answering what it
    /// answers unless its host has given the invocation up by then.
    fn answer<T>(
        &mut self,
        memory: &mut wiggle::GuestMemory<'_>,
        call: impl FnOnce(&mut GuestMemory<'_, '_>, &mut Invocation) -> Answer<T>,
    ) -> Answer<T> {
        let fuel = self.hostcall_fuel();
        let answer = call(&mut GuestMemory::new(memory, fuel), self);
        match self.abandonment() {
            Some(trap) => Err(types::Error::trap(trap)),
            None => answer,
        }
    }
}

impl wasi_snapshot_preview1::WasiSnapshotPreview1 for Invocation {
    fn set_hostcall_fuel(&mut self, fuel: usize) {
        Invocation::set_hostcall_fuel(self, fuel);
    }

    fn fd_advise(
        &mut self,
        memory: &mut wiggle::GuestMemory<'_>,
        fd: types::Fd,
        offset: types::Filesize,
        len: types::Filesize,
        _advice: types::Advice,
    ) -> Answer {
        self.answer(memory, |_, inv| {
            fd::advise(inv.wasi_mut(), fd.into(), offset, len)
        })
    }

    fn fd_allocate(
        &mut self,
        memory: &mut wiggle::GuestMemory<'_>,
        fd: types::Fd,
        offset: types::Filesize,
        len: types::Filesize,
    ) -> Answer {
        self.answer(memory, |_, inv| {
            fd::allocate(inv.wasi_mut(), fd.into(), offset, len)
        })
    }

    fn fd_close(&mut self, memory: &mut wiggle::GuestMemory<'_>, fd: types::Fd) -> Answer {
        self.answer(memory, |_, inv| fd::close(inv.wasi_mut(), fd.into()))
    }

    fn fd_datasync(&mut self, memory: &mut wiggle::GuestMemory<'_>, fd: types::Fd) -> Answer {
        self.answer(memory, |_, inv| fd::sync(inv.wasi_mut(), fd.into(), true))
    }

    fn fd_fdstat_get(
        &mut self,
        memory: &mut wiggle::GuestMemory<'_>,
        fd: types::Fd,
    ) -> Answer<types::Fdstat> {
        self.answer(memory, |_, inv| fd::fdstat_get(inv.wasi_mut(), fd.into()))
    }

    fn fd_fdstat_set_flags(
        &mut self,
        memory: &mut wiggle::GuestMemory<'_>,
        fd: types::Fd,
        flags: types::Fdflags,
    ) -> Answer {
        self.answer(memory, |_, inv| {
            fd::fdstat_set_flags(inv.wasi_mut(), fd.into(), flags)
        })
    }

    fn fd_fdstat_set_rights(
        &mut self,
        memory: &mut wiggle::GuestMemory<'_>,
        fd: types::Fd,
        _fs_rights_base: types::Rights,
        _fs_rights_inheriting: types::Rights,
    ) -> Answer {
        self.answer(memory, |_, inv| {
            fd::fdstat_set_rights(inv.wasi_mut(), fd.into())
        })
    }

    fn fd_filestat_get(
        &mut self,
        memory: &mut wiggle::GuestMemory<'_>,
        fd: types::Fd,
    ) -> Answer<types::Filestat> {
        self.answer(memory, |_, inv| fd::filestat_get(inv.wasi(), fd.into()))
    }

    fn fd_filestat_set_size(
        &mut self,
        memory: &mut wiggle::GuestMemory<'_>,
        fd: types::Fd,
        size: types::Filesize,
    ) -> Answer {
        self.answer(memory, |_, inv| {
            fd::filestat_set_size(inv.wasi_mut(), fd.into(), size)
        })
    }

    fn fd_filestat_set_times(
        &mut self,
        memory: &mut wiggle::GuestMemory<'_>,
        fd: types::Fd,
        atim: types::Timestamp,
        mtim: types::Timestamp,
        fst_flags: types::Fstflags,
    ) -> Answer {
        self.answer(memory, |_, inv| {
            fd::utimensat(atim, mtim, fst_flags, |times| {
                fd::filestat_set_times(inv.wasi(), fd.into(), times)
            })
        })
    }

    fn fd_pread(
        &mut self,
        memory: &mut wiggle::GuestMemory<'_>,
        fd: types::Fd,
        iovs: types::IovecArray,
        offset: types::Filesize,
    ) -> Answer<types::Size> {
        self.answer(memory, |mem, inv| {
            fd::pread(inv.wasi_mut(), mem, fd.into(), iovs, offset)
        })
    }

    fn fd_prestat_get(
        &mut self,
        memory: &mut wiggle::GuestMemory<'_>,
        fd: types::Fd,
    ) -> Answer<types::Prestat> {
        self.answer(memory, |_, inv| fd::prestat_get(inv.wasi_mut(), fd.into()))
    }

    fn fd_prestat_dir_name(
        &mut self,
        memory: &mut wiggle::GuestMemory<'_>,
        fd: types::Fd,
        path: GuestPtr<u8>,
        path_max_len: types::Size,
    ) -> Answer {
        self.answer(memory, |mem, inv| {
            fd::prestat_dir_name(inv.wasi_mut(), mem, fd.into(), path, path_max_len)
        })
    }

    fn fd_pwrite(
        &mut self,
        memory: &mut wiggle::GuestMemory<'_>,
        fd: types::Fd,
        ciovs: types::CiovecArray,
        offset: types::Filesize,
    ) -> Answer<types::Size> {
        self.answer(memory, |mem, inv| {
            fd::pwrite(inv.wasi_mut(), mem, fd.into(), ciovs, offset)
        })
    }

    fn fd_read(
        &mut self,
        memory: &mut wiggle::GuestMemory<'_>,
        fd: types::Fd,
        iovs: types::IovecArray,
    ) -> Answer<types::Size> {
        self.answer(memory, |mem, inv| {
            fd::read(inv.wasi_mut(), mem, fd.into(), iovs)
        })
    }

    fn fd_readdir(
        &mut self,
        memory: &mut wiggle::GuestMemory<'_>,
        fd: types::Fd,
        buf: GuestPtr<u8>,
        buf_len: types::Size,
        cookie: types::Dircookie,
    ) -> Answer<types::Size> {
        self.answer(memory, |mem, inv| {
            fd::readdir(inv.wasi_mut(), mem, fd.into(), buf, buf_len, cookie)
        })
    }

    fn fd_renumber(
        &mut self,
        memory: &mut wiggle::GuestMemory<'_>,
        from: types::Fd,
        to: types::Fd,
    ) -> Answer {
        self.answer(memory, |_, inv| {
            fd::renumber(inv.wasi_mut(), from.into(), to.into())
        })
    }

    fn fd_seek(
        &mut self,
        memory: &mut wiggle::GuestMemory<'_>,
        fd: types::Fd,
        offset: types::Filedelta,
        whence: types::Whence,
    ) -> Answer<types::Filesize> {
        self.answer(memory, |_, inv| {
            fd::seek(inv.wasi_mut(), fd.into(), offset, whence)
        })
    }

    fn fd_sync(&mut self, memory: &mut wiggle::GuestMemory<'_>, fd: types::Fd) -> Answer {
        self.answer(memory, |_, inv| fd::sync(inv.wasi_mut(), fd.into(), false))
    }

    fn fd_tell(
        &mut self,
        memory: &mut wiggle::GuestMemory<'_>,
        fd: types::Fd,
    ) -> Answer<types::Filesize> {
        self.answer(memory, |_, inv| fd::tell(inv.wasi_mut(), fd.into()))
    }

    fn fd_write(
        &mut self,
        memory: &mut wiggle::GuestMemory<'_>,
        fd: types::Fd,
        ciovs: types::CiovecArray,
    ) -> Answer<types::Size> {
        self.answer(memory, |mem, inv| {
            fd::write(inv.wasi_mut(), mem, fd.into(), ciovs)
        })
    }

    fn path_create_directory(
        &mut self,
        memory: &mut wiggle::GuestMemory<'_>,
        fd: types::Fd,
        path: GuestPtr<str>,
    ) -> Answer {
        self.answer(memory, |mem, inv| {
            path::create_directory(inv.wasi(), mem, fd.into(), path)
        })
    }

    fn path_filestat_get(
        &mut self,
        memory: &mut wiggle::GuestMemory<'_>,
        fd: types::Fd,
        flags: types::Lookupflags,
        path: GuestPtr<str>,
    ) -> Answer<types::Filestat> {
        self.answer(memory, |mem, inv| {
            path::filestat_get(inv.wasi(), mem, fd.into(), flags, path)
        })
    }

    fn path_filestat_set_times(
        &mut self,
        memory: &mut wiggle::GuestMemory<'_>,
        fd: types::Fd,
        flags: types::Lookupflags,
        path: GuestPtr<str>,
        atim: types::Timestamp,
        mtim: types::Timestamp,
        fst_flags: types::Fstflags,
    ) -> Answer {
        self.answer(memory, |mem, inv| {
            fd::utimensat(atim, mtim, fst_flags, |times| {
                path::filestat_set_times(inv.wasi(), mem, fd.into(), flags, path, times)
            })
        })
    }

    fn path_link(
        &mut self,
        memory: &mut wiggle::GuestMemory<'_>,
        old_fd: types::Fd,
        old_flags: types::Lookupflags,
        old_path: GuestPtr<str>,
        new_fd: types::Fd,
        new_path: GuestPtr<str>,
    ) -> Answer {
        self.answer(memory, |mem, inv| {
            path::link(
                inv.wasi_mut(),
                mem,
                (old_fd.into(), old_path),
                (new_fd.into(), new_path),
                old_flags,
            )
        })
    }

    fn path_open(
        &mut self,
        memory: &mut wiggle::GuestMemory<'_>,
        fd: types::Fd,
        dirflags: types::Lookupflags,
        path: GuestPtr<str>,
        oflags: types::Oflags,
        fs_rights_base: types::Rights,
        _fs_rights_inheriting: types::Rights,
        fdflags: types::Fdflags,
    ) -> Answer<types::Fd> {
        self.answer(memory, |mem, inv| {
            path::open(
                inv.wasi_mut(),
                mem,
                fd.into(),
                path,
                &path::PathOpen {
                    dirflags,
                    oflags,
                    rights_base: fs_rights_base,
                    fdflags,
                },
            )
        })
    }

    fn path_readlink(
        &mut self,
        memory: &mut wiggle::GuestMemory<'_>,
        fd: types::Fd,
        path: GuestPtr<str>,
        buf: GuestPtr<u8>,
        buf_len: types::Size,
    ) -> Answer<types::Size> {
        self.answer(memory, |mem, inv| {
            path::readlink(inv.wasi(), mem, fd.into(), path, buf, buf_len)
        })
    }

    fn path_remove_directory(
        &mut self,
        memory: &mut wiggle::GuestMemory<'_>,
        fd: types::Fd,
        path: GuestPtr<str>,
    ) -> Answer {
        self.answer(memory, |mem, inv| {
            path::remove_directory(inv.wasi(), mem, fd.into(), path)
        })
    }

    fn path_rename(
        &mut self,
        memory: &mut wiggle::GuestMemory<'_>,
        old_fd: types::Fd,
        old_path: GuestPtr<str>,
        new_fd: types::Fd,
        new_path: GuestPtr<str>,
    ) -> Answer {
        self.answer(memory, |mem, inv| {
            path::rename(
                inv.wasi_mut(),
                mem,
                (old_fd.into(), old_path),
                (new_fd.into(), new_path),
            )
        })
    }

    fn path_symlink(
        &mut self,
        memory: &mut wiggle::GuestMemory<'_>,
        old_path: GuestPtr<str>,
        fd: types::Fd,
        new_path: GuestPtr<str>,
    ) -> Answer {
        self.answer(memory, |mem, inv| {
            path::symlink(inv.wasi(), mem, old_path, fd.into(), new_path)
        })
    }

    fn path_unlink_file(
        &mut self,
        memory: &mut wiggle::GuestMemory<'_>,
        fd: types::Fd,
        path: GuestPtr<str>,
    ) -> Answer {
        self.answer(memory, |mem, inv| {
            path::unlink_file(inv.wasi(), mem, fd.into(), path)
        })
    }

    fn poll_oneoff(
        &mut self,
        memory: &mut wiggle::GuestMemory<'_>,
        subs: GuestPtr<types::Subscription>,
        events: GuestPtr<types::Event>,
        count: types::Size,
    ) -> Answer<types::Size> {
        self.answer(memory, |mem, inv| {
            poll::poll_oneoff(inv, mem, subs, events, count)
        })
    }

    fn sock_accept(
        &mut self,
        memory: &mut wiggle::GuestMemory<'_>,
        fd: types::Fd,
        _flags: types::Fdflags,
    ) -> Answer<types::Fd> {
        self.answer(memory, |_, inv| {
            sock::refuse_socket_call(inv.wasi_mut(), fd.into())
        })
    }

    fn sock_recv(
        &mut self,
        memory: &mut wiggle::GuestMemory<'_>,
        fd: types::Fd,
        _ri_data: types::IovecArray,
        _ri_flags: types::Riflags,
    ) -> Answer<(types::Size, types::Roflags)> {
        self.answer(memory, |_, inv| {
            sock::refuse_socket_call(inv.wasi_mut(), fd.into())
        })
    }

    fn sock_send(
        &mut self,
        memory: &mut wiggle::GuestMemory<'_>,
        fd: types::Fd,
        _si_data: types::CiovecArray,
        _si_flags: types::Siflags,
    ) -> Answer<types::Size> {
        self.answer(memory, |_, inv| {
            sock::refuse_socket_call(inv.wasi_mut(), fd.into())
        })
    }

    fn sock_shutdown(
        &mut self,
        memory: &mut wiggle::GuestMemory<'_>,
        fd: types::Fd,
        _how: types::Sdflags,
    ) -> Answer {
        self.answer(memory, |_, inv| {
            sock::refuse_socket_call(inv.wasi_mut(), fd.into())
        })
    }
}
