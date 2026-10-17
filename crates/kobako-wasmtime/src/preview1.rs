//! kobako's own answers to the 36 preview1 imports that read the
//! descriptor table: `fd_*`, `path_*`, `sock_*`, and `poll_oneoff`.
//!
//! wasmtime-wasi keeps its table private, so a descriptor kobako places
//! there would stay invisible to its calls. Answering every call that reads
//! a descriptor keeps the guest on one table, `crate::descriptors`. The
//! other ten imports — arguments, environment, clocks, process, scheduling,
//! and randomness — stay wasmtime-wasi's.

mod args;
mod errno;
mod fd;
mod memory;
mod path;
mod poll;
mod sock;

#[cfg(test)]
mod differential;

use wasmtime::{Caller, Extern, Linker};

use crate::invocation::Invocation;
use errno::{Answer, Failure};
use memory::GuestMemory;
use path::OpenFlags;

const MODULE: &str = "wasi_snapshot_preview1";

/// Run one call against the calling instance's memory and invocation,
/// answering its `errno`, or the trap that ends the invocation.
fn answer(
    caller: &mut Caller<'_, Invocation>,
    call: impl FnOnce(&mut GuestMemory, &mut Invocation) -> Answer,
) -> wasmtime::Result<i32> {
    let memory = caller
        .get_export("memory")
        .and_then(Extern::into_memory)
        .ok_or_else(|| wasmtime::Error::msg("a preview1 call needs the guest's `memory` export"))?;
    let (bytes, invocation) = memory.data_and_store_mut(caller);
    match call(&mut GuestMemory::new(bytes), invocation) {
        Ok(()) => Ok(0),
        Err(Failure::Errno(errno)) => Ok(errno as i32),
        Err(Failure::Trap(trap)) => Err(trap),
    }
}

/// Define kobako's answers over wasmtime-wasi's for the same names.
pub(crate) fn shadow(linker: &mut Linker<Invocation>) -> wasmtime::Result<()> {
    linker.allow_shadowing(true);
    shadow_fd(linker)?;
    shadow_path(linker)?;
    shadow_poll_and_sock(linker)?;
    linker.allow_shadowing(false);
    Ok(())
}

fn shadow_fd(linker: &mut Linker<Invocation>) -> wasmtime::Result<()> {
    type C<'a> = Caller<'a, Invocation>;
    linker.func_wrap(
        MODULE,
        "fd_advise",
        |mut c: C, fd: i32, _: i64, _: i64, advice: i32| {
            answer(&mut c, |_, inv| {
                fd::advise(inv.wasi().descriptors(), fd as u32, advice as u32)
            })
        },
    )?;
    linker.func_wrap(
        MODULE,
        "fd_allocate",
        |mut c: C, fd: i32, _: i64, _: i64| {
            answer(&mut c, |_, inv| {
                fd::allocate(inv.wasi().descriptors(), fd as u32)
            })
        },
    )?;
    linker.func_wrap(MODULE, "fd_close", |mut c: C, fd: i32| {
        answer(&mut c, |_, inv| {
            fd::close(inv.wasi_mut().descriptors_mut(), fd as u32)
        })
    })?;
    linker.func_wrap(MODULE, "fd_datasync", |mut c: C, fd: i32| {
        answer(&mut c, |_, inv| {
            fd::datasync(inv.wasi().descriptors(), fd as u32)
        })
    })?;
    linker.func_wrap(MODULE, "fd_fdstat_get", |mut c: C, fd: i32, out: i32| {
        answer(&mut c, |mem, inv| {
            fd::fdstat_get(inv.wasi().descriptors(), mem, fd as u32, out as u32)
        })
    })?;
    linker.func_wrap(
        MODULE,
        "fd_fdstat_set_flags",
        |mut c: C, fd: i32, flags: i32| {
            answer(&mut c, |_, inv| {
                fd::fdstat_set_flags(inv.wasi().descriptors(), fd as u32, flags as u32)
            })
        },
    )?;
    linker.func_wrap(
        MODULE,
        "fd_fdstat_set_rights",
        |mut c: C, fd: i32, base: i64, inheriting: i64| {
            answer(&mut c, |_, inv| {
                fd::fdstat_set_rights(
                    inv.wasi().descriptors(),
                    fd as u32,
                    base as u64,
                    inheriting as u64,
                )
            })
        },
    )?;
    linker.func_wrap(MODULE, "fd_filestat_get", |mut c: C, fd: i32, out: i32| {
        answer(&mut c, |mem, inv| {
            fd::filestat_get(inv.wasi().descriptors(), mem, fd as u32, out as u32)
        })
    })?;
    linker.func_wrap(
        MODULE,
        "fd_filestat_set_size",
        |mut c: C, fd: i32, _: i64| {
            answer(&mut c, |_, inv| {
                fd::filestat_set_size(inv.wasi().descriptors(), fd as u32)
            })
        },
    )?;
    linker.func_wrap(
        MODULE,
        "fd_filestat_set_times",
        |mut c: C, fd: i32, _: i64, _: i64, fst_flags: i32| {
            answer(&mut c, |_, inv| {
                fd::filestat_set_times(inv.wasi().descriptors(), fd as u32, fst_flags as u32)
            })
        },
    )?;
    linker.func_wrap(
        MODULE,
        "fd_pread",
        |mut c: C, fd: i32, iovs: i32, iovs_len: i32, _: i64, _: i32| {
            answer(&mut c, |mem, inv| {
                fd::pread(
                    inv.wasi().descriptors(),
                    mem,
                    fd as u32,
                    iovs as u32,
                    iovs_len as u32,
                )
            })
        },
    )?;
    linker.func_wrap(MODULE, "fd_prestat_get", |mut c: C, fd: i32, _: i32| {
        answer(&mut c, |_, inv| {
            fd::prestat_get(inv.wasi().descriptors(), fd as u32)
        })
    })?;
    linker.func_wrap(
        MODULE,
        "fd_prestat_dir_name",
        |mut c: C, fd: i32, _: i32, _: i32| {
            answer(&mut c, |_, inv| {
                fd::prestat_dir_name(inv.wasi().descriptors(), fd as u32)
            })
        },
    )?;
    linker.func_wrap(
        MODULE,
        "fd_pwrite",
        |mut c: C, fd: i32, ciovs: i32, ciovs_len: i32, _: i64, _: i32| {
            answer(&mut c, |mem, inv| {
                fd::pwrite(
                    inv.wasi().descriptors(),
                    mem,
                    fd as u32,
                    ciovs as u32,
                    ciovs_len as u32,
                )
            })
        },
    )?;
    linker.func_wrap(
        MODULE,
        "fd_read",
        |mut c: C, fd: i32, iovs: i32, iovs_len: i32, nread: i32| {
            answer(&mut c, |mem, inv| {
                fd::read(
                    inv.wasi_mut().descriptors_mut(),
                    mem,
                    fd as u32,
                    iovs as u32,
                    iovs_len as u32,
                    nread as u32,
                )
            })
        },
    )?;
    linker.func_wrap(
        MODULE,
        "fd_readdir",
        |mut c: C, fd: i32, _: i32, _: i32, _: i64, _: i32| {
            answer(&mut c, |_, inv| {
                fd::readdir(inv.wasi().descriptors(), fd as u32)
            })
        },
    )?;
    linker.func_wrap(MODULE, "fd_renumber", |mut c: C, from: i32, to: i32| {
        answer(&mut c, |_, inv| {
            fd::renumber(inv.wasi_mut().descriptors_mut(), from as u32, to as u32)
        })
    })?;
    linker.func_wrap(
        MODULE,
        "fd_seek",
        |mut c: C, fd: i32, _: i64, whence: i32, _: i32| {
            answer(&mut c, |_, inv| {
                fd::seek(inv.wasi().descriptors(), fd as u32, whence as u32)
            })
        },
    )?;
    linker.func_wrap(MODULE, "fd_sync", |mut c: C, fd: i32| {
        answer(&mut c, |_, inv| {
            fd::sync(inv.wasi().descriptors(), fd as u32)
        })
    })?;
    linker.func_wrap(MODULE, "fd_tell", |mut c: C, fd: i32, _: i32| {
        answer(&mut c, |_, inv| {
            fd::tell(inv.wasi().descriptors(), fd as u32)
        })
    })?;
    linker.func_wrap(
        MODULE,
        "fd_write",
        |mut c: C, fd: i32, ciovs: i32, ciovs_len: i32, nwritten: i32| {
            answer(&mut c, |mem, inv| {
                fd::write(
                    inv.wasi_mut().descriptors_mut(),
                    mem,
                    fd as u32,
                    ciovs as u32,
                    ciovs_len as u32,
                    nwritten as u32,
                )
            })
        },
    )?;
    Ok(())
}

fn shadow_path(linker: &mut Linker<Invocation>) -> wasmtime::Result<()> {
    type C<'a> = Caller<'a, Invocation>;
    linker.func_wrap(
        MODULE,
        "path_create_directory",
        |mut c: C, fd: i32, _: i32, _: i32| {
            answer(&mut c, |_, inv| {
                path::create_directory(inv.wasi().descriptors(), fd as u32)
            })
        },
    )?;
    linker.func_wrap(
        MODULE,
        "path_filestat_get",
        |mut c: C, fd: i32, flags: i32, _: i32, _: i32, _: i32| {
            answer(&mut c, |_, inv| {
                path::filestat_get(inv.wasi().descriptors(), fd as u32, flags as u32)
            })
        },
    )?;
    linker.func_wrap(
        MODULE,
        "path_filestat_set_times",
        |mut c: C, fd: i32, flags: i32, _: i32, _: i32, _: i64, _: i64, fst_flags: i32| {
            answer(&mut c, |_, inv| {
                path::filestat_set_times(
                    inv.wasi().descriptors(),
                    fd as u32,
                    flags as u32,
                    fst_flags as u32,
                )
            })
        },
    )?;
    linker.func_wrap(
        MODULE,
        "path_link",
        |mut c: C, old_fd: i32, old_flags: i32, _: i32, _: i32, _: i32, _: i32, _: i32| {
            answer(&mut c, |_, inv| {
                path::link(inv.wasi().descriptors(), old_fd as u32, old_flags as u32)
            })
        },
    )?;
    linker.func_wrap(
        MODULE,
        "path_open",
        |mut c: C,
         fd: i32,
         dirflags: i32,
         path: i32,
         path_len: i32,
         oflags: i32,
         rights_base: i64,
         rights_inheriting: i64,
         fdflags: i32,
         _: i32| {
            let flags = OpenFlags {
                dirflags: dirflags as u32,
                oflags: oflags as u32,
                rights_base: rights_base as u64,
                rights_inheriting: rights_inheriting as u64,
                fdflags: fdflags as u32,
            };
            answer(&mut c, |mem, inv| {
                path::open(
                    inv.wasi().descriptors(),
                    mem,
                    fd as u32,
                    path as u32,
                    path_len as u32,
                    &flags,
                )
            })
        },
    )?;
    linker.func_wrap(
        MODULE,
        "path_readlink",
        |mut c: C, fd: i32, _: i32, _: i32, _: i32, _: i32, _: i32| {
            answer(&mut c, |_, inv| {
                path::readlink(inv.wasi().descriptors(), fd as u32)
            })
        },
    )?;
    linker.func_wrap(
        MODULE,
        "path_remove_directory",
        |mut c: C, fd: i32, _: i32, _: i32| {
            answer(&mut c, |_, inv| {
                path::remove_directory(inv.wasi().descriptors(), fd as u32)
            })
        },
    )?;
    linker.func_wrap(
        MODULE,
        "path_rename",
        |mut c: C, fd: i32, _: i32, _: i32, _: i32, _: i32, _: i32| {
            answer(&mut c, |_, inv| {
                path::rename(inv.wasi().descriptors(), fd as u32)
            })
        },
    )?;
    linker.func_wrap(
        MODULE,
        "path_symlink",
        |mut c: C, _: i32, _: i32, fd: i32, _: i32, _: i32| {
            answer(&mut c, |_, inv| {
                path::symlink(inv.wasi().descriptors(), fd as u32)
            })
        },
    )?;
    linker.func_wrap(
        MODULE,
        "path_unlink_file",
        |mut c: C, fd: i32, _: i32, _: i32| {
            answer(&mut c, |_, inv| {
                path::unlink_file(inv.wasi().descriptors(), fd as u32)
            })
        },
    )?;
    Ok(())
}

fn shadow_poll_and_sock(linker: &mut Linker<Invocation>) -> wasmtime::Result<()> {
    type C<'a> = Caller<'a, Invocation>;
    linker.func_wrap(
        MODULE,
        "poll_oneoff",
        |mut c: C, subs: i32, events: i32, count: i32, nevents: i32| {
            answer(&mut c, |mem, inv| {
                poll::poll_oneoff(
                    inv,
                    mem,
                    subs as u32,
                    events as u32,
                    count as u32,
                    nevents as u32,
                )
            })
        },
    )?;
    linker.func_wrap(
        MODULE,
        "sock_accept",
        |mut c: C, fd: i32, flags: i32, _: i32| {
            answer(&mut c, |_, inv| {
                sock::accept(inv.wasi().descriptors(), fd as u32, flags as u32)
            })
        },
    )?;
    linker.func_wrap(
        MODULE,
        "sock_recv",
        |mut c: C, fd: i32, _: i32, _: i32, ri_flags: i32, _: i32, _: i32| {
            answer(&mut c, |_, inv| {
                sock::recv(inv.wasi().descriptors(), fd as u32, ri_flags as u32)
            })
        },
    )?;
    linker.func_wrap(
        MODULE,
        "sock_send",
        |mut c: C, fd: i32, _: i32, _: i32, _: i32, _: i32| {
            answer(&mut c, |_, inv| {
                sock::send(inv.wasi().descriptors(), fd as u32)
            })
        },
    )?;
    linker.func_wrap(MODULE, "sock_shutdown", |mut c: C, fd: i32, how: i32| {
        answer(&mut c, |_, inv| {
            sock::shutdown(inv.wasi().descriptors(), fd as u32, how as u32)
        })
    })?;
    Ok(())
}
