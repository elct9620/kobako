//! kobako's own answers to the 36 preview1 imports that read the
//! descriptor table: `fd_*`, `path_*`, `sock_*`, and `poll_oneoff`.
//!
//! wasmtime-wasi keeps its table private, so a descriptor kobako places
//! there would stay invisible to its calls. Answering every call that reads
//! a descriptor keeps the guest on one table, `crate::descriptors`, where
//! each filesystem Grant is a preopen. The other ten imports — arguments,
//! environment, clocks, process, scheduling, and randomness — stay
//! wasmtime-wasi's.

mod args;
mod beneath;
mod errno;
mod fd;
mod memory;
mod path;
mod poll;
mod sock;

#[cfg(test)]
mod differential;
#[cfg(test)]
mod granted;

use wasmtime::{Caller, Extern, Linker};

use crate::descriptors::Descriptors;
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

fn table(invocation: &mut Invocation) -> &mut Descriptors {
    invocation.wasi_mut().descriptors_mut()
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
                fd::advise(table(inv), fd as u32, advice as u32)
            })
        },
    )?;
    linker.func_wrap(
        MODULE,
        "fd_allocate",
        |mut c: C, fd: i32, _: i64, _: i64| {
            answer(&mut c, |_, inv| fd::allocate(table(inv), fd as u32))
        },
    )?;
    linker.func_wrap(MODULE, "fd_close", |mut c: C, fd: i32| {
        answer(&mut c, |_, inv| fd::close(table(inv), fd as u32))
    })?;
    // A Filesystem makes what was written durable in one step, so data
    // and metadata sync alike.
    for name in ["fd_datasync", "fd_sync"] {
        linker.func_wrap(MODULE, name, |mut c: C, fd: i32| {
            answer(&mut c, |_, inv| fd::sync(table(inv), fd as u32))
        })?;
    }
    linker.func_wrap(MODULE, "fd_fdstat_get", |mut c: C, fd: i32, out: i32| {
        answer(&mut c, |mem, inv| {
            fd::fdstat_get(table(inv), mem, fd as u32, out as u32)
        })
    })?;
    linker.func_wrap(
        MODULE,
        "fd_fdstat_set_flags",
        |mut c: C, fd: i32, flags: i32| {
            answer(&mut c, |_, inv| {
                fd::fdstat_set_flags(table(inv), fd as u32, flags as u32)
            })
        },
    )?;
    linker.func_wrap(
        MODULE,
        "fd_fdstat_set_rights",
        |mut c: C, fd: i32, base: i64, inheriting: i64| {
            answer(&mut c, |_, inv| {
                fd::fdstat_set_rights(table(inv), fd as u32, base as u64, inheriting as u64)
            })
        },
    )?;
    linker.func_wrap(MODULE, "fd_filestat_get", |mut c: C, fd: i32, out: i32| {
        answer(&mut c, |mem, inv| {
            fd::filestat_get(table(inv), mem, fd as u32, out as u32)
        })
    })?;
    linker.func_wrap(
        MODULE,
        "fd_filestat_set_size",
        |mut c: C, fd: i32, size: i64| {
            answer(&mut c, |_, inv| {
                fd::filestat_set_size(table(inv), fd as u32, size as u64)
            })
        },
    )?;
    linker.func_wrap(
        MODULE,
        "fd_filestat_set_times",
        |mut c: C, fd: i32, _: i64, _: i64, fst_flags: i32| {
            answer(&mut c, |_, inv| {
                fd::filestat_set_times(table(inv), fd as u32, fst_flags as u32)
            })
        },
    )?;
    linker.func_wrap(
        MODULE,
        "fd_pread",
        |mut c: C, fd: i32, iovs: i32, iovs_len: i32, offset: i64, nread: i32| {
            answer(&mut c, |mem, inv| {
                let iovs = (iovs as u32, iovs_len as u32);
                fd::pread(
                    table(inv),
                    mem,
                    fd as u32,
                    iovs,
                    offset as u64,
                    nread as u32,
                )
            })
        },
    )?;
    linker.func_wrap(MODULE, "fd_prestat_get", |mut c: C, fd: i32, out: i32| {
        answer(&mut c, |mem, inv| {
            fd::prestat_get(table(inv), mem, fd as u32, out as u32)
        })
    })?;
    linker.func_wrap(
        MODULE,
        "fd_prestat_dir_name",
        |mut c: C, fd: i32, path: i32, path_len: i32| {
            answer(&mut c, |mem, inv| {
                fd::prestat_dir_name(table(inv), mem, fd as u32, path as u32, path_len as u32)
            })
        },
    )?;
    linker.func_wrap(
        MODULE,
        "fd_pwrite",
        |mut c: C, fd: i32, ciovs: i32, ciovs_len: i32, offset: i64, nwritten: i32| {
            answer(&mut c, |mem, inv| {
                let ciovs = (ciovs as u32, ciovs_len as u32);
                fd::pwrite(
                    table(inv),
                    mem,
                    fd as u32,
                    ciovs,
                    offset as u64,
                    nwritten as u32,
                )
            })
        },
    )?;
    linker.func_wrap(
        MODULE,
        "fd_read",
        |mut c: C, fd: i32, iovs: i32, iovs_len: i32, nread: i32| {
            answer(&mut c, |mem, inv| {
                let iovs = (iovs as u32, iovs_len as u32);
                fd::read(table(inv), mem, fd as u32, iovs, nread as u32)
            })
        },
    )?;
    linker.func_wrap(
        MODULE,
        "fd_readdir",
        |mut c: C, fd: i32, buf: i32, buf_len: i32, cookie: i64, bufused: i32| {
            answer(&mut c, |mem, inv| {
                let buf = (buf as u32, buf_len as u32);
                fd::readdir(
                    table(inv),
                    mem,
                    fd as u32,
                    buf,
                    cookie as u64,
                    bufused as u32,
                )
            })
        },
    )?;
    linker.func_wrap(MODULE, "fd_renumber", |mut c: C, from: i32, to: i32| {
        answer(&mut c, |_, inv| {
            fd::renumber(table(inv), from as u32, to as u32)
        })
    })?;
    linker.func_wrap(
        MODULE,
        "fd_seek",
        |mut c: C, fd: i32, offset: i64, whence: i32, out: i32| {
            answer(&mut c, |mem, inv| {
                fd::seek(
                    table(inv),
                    mem,
                    fd as u32,
                    offset,
                    whence as u32,
                    out as u32,
                )
            })
        },
    )?;
    linker.func_wrap(MODULE, "fd_tell", |mut c: C, fd: i32, out: i32| {
        answer(&mut c, |mem, inv| {
            fd::tell(table(inv), mem, fd as u32, out as u32)
        })
    })?;
    linker.func_wrap(
        MODULE,
        "fd_write",
        |mut c: C, fd: i32, ciovs: i32, ciovs_len: i32, nwritten: i32| {
            answer(&mut c, |mem, inv| {
                let ciovs = (ciovs as u32, ciovs_len as u32);
                fd::write(table(inv), mem, fd as u32, ciovs, nwritten as u32)
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
        |mut c: C, fd: i32, path: i32, len: i32| {
            answer(&mut c, |mem, inv| {
                path::create_directory(table(inv), mem, fd as u32, (path as u32, len as u32))
            })
        },
    )?;
    linker.func_wrap(
        MODULE,
        "path_filestat_get",
        |mut c: C, fd: i32, flags: i32, path: i32, len: i32, out: i32| {
            answer(&mut c, |mem, inv| {
                let path = (path as u32, len as u32);
                path::filestat_get(table(inv), mem, fd as u32, flags as u32, path, out as u32)
            })
        },
    )?;
    linker.func_wrap(
        MODULE,
        "path_filestat_set_times",
        |mut c: C, fd: i32, flags: i32, path: i32, len: i32, _: i64, _: i64, fst_flags: i32| {
            answer(&mut c, |mem, inv| {
                let path = (path as u32, len as u32);
                path::filestat_set_times(
                    table(inv),
                    mem,
                    fd as u32,
                    flags as u32,
                    path,
                    fst_flags as u32,
                )
            })
        },
    )?;
    linker.func_wrap(
        MODULE,
        "path_link",
        |mut c: C,
         old_fd: i32,
         old_flags: i32,
         old_path: i32,
         old_len: i32,
         new_fd: i32,
         new_path: i32,
         new_len: i32| {
            answer(&mut c, |mem, inv| {
                path::link(
                    table(inv),
                    mem,
                    old_fd as u32,
                    old_flags as u32,
                    (old_path as u32, old_len as u32),
                    new_fd as u32,
                    (new_path as u32, new_len as u32),
                )
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
         out: i32| {
            let flags = OpenFlags {
                dirflags: dirflags as u32,
                oflags: oflags as u32,
                rights_base: rights_base as u64,
                rights_inheriting: rights_inheriting as u64,
                fdflags: fdflags as u32,
            };
            answer(&mut c, |mem, inv| {
                let path = (path as u32, path_len as u32);
                path::open(table(inv), mem, fd as u32, path, &flags, out as u32)
            })
        },
    )?;
    linker.func_wrap(
        MODULE,
        "path_readlink",
        |mut c: C, fd: i32, path: i32, len: i32, _: i32, _: i32, _: i32| {
            answer(&mut c, |mem, inv| {
                path::readlink(table(inv), mem, fd as u32, (path as u32, len as u32))
            })
        },
    )?;
    linker.func_wrap(
        MODULE,
        "path_remove_directory",
        |mut c: C, fd: i32, path: i32, len: i32| {
            answer(&mut c, |mem, inv| {
                path::remove_directory(table(inv), mem, fd as u32, (path as u32, len as u32))
            })
        },
    )?;
    linker.func_wrap(
        MODULE,
        "path_rename",
        |mut c: C, fd: i32, old: i32, old_len: i32, new_fd: i32, new: i32, new_len: i32| {
            answer(&mut c, |mem, inv| {
                path::rename(
                    table(inv),
                    mem,
                    (fd as u32, (old as u32, old_len as u32)),
                    (new_fd as u32, (new as u32, new_len as u32)),
                )
            })
        },
    )?;
    linker.func_wrap(
        MODULE,
        "path_symlink",
        |mut c: C, old: i32, old_len: i32, fd: i32, new: i32, new_len: i32| {
            answer(&mut c, |mem, inv| {
                path::symlink(
                    table(inv),
                    mem,
                    (old as u32, old_len as u32),
                    fd as u32,
                    (new as u32, new_len as u32),
                )
            })
        },
    )?;
    linker.func_wrap(
        MODULE,
        "path_unlink_file",
        |mut c: C, fd: i32, path: i32, len: i32| {
            answer(&mut c, |mem, inv| {
                path::unlink_file(table(inv), mem, fd as u32, (path as u32, len as u32))
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
                sock::accept(table(inv), fd as u32, flags as u32)
            })
        },
    )?;
    linker.func_wrap(
        MODULE,
        "sock_recv",
        |mut c: C, fd: i32, _: i32, _: i32, ri_flags: i32, _: i32, _: i32| {
            answer(&mut c, |_, inv| {
                sock::recv(table(inv), fd as u32, ri_flags as u32)
            })
        },
    )?;
    linker.func_wrap(
        MODULE,
        "sock_send",
        |mut c: C, fd: i32, _: i32, _: i32, _: i32, _: i32| {
            answer(&mut c, |_, inv| sock::send(table(inv), fd as u32))
        },
    )?;
    linker.func_wrap(MODULE, "sock_shutdown", |mut c: C, fd: i32, how: i32| {
        answer(&mut c, |_, inv| {
            sock::shutdown(table(inv), fd as u32, how as u32)
        })
    })?;
    Ok(())
}
