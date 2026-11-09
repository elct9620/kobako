# Preview1 Encoding

The Wasm Driver encodes each WASI preview1 file call onto the VFS in `kobako-runtime`, which decides every answer a guest sees. The Wasm Driver keeps only what preview1 itself defines and what a Linux syscall entry checks before the VFS. The layers are in [`README.md`](README.md); why the split sits there is in [`../isolation/README.md`](../isolation/README.md).

```
+-------+  preview1  +-------------+  asks  +-----+  asks  +------------+
| guest |----------->| Wasm Driver |------->| VFS |------->| Filesystem |
+-------+            +-------------+        +-----+        +------------+
                       encodes                decides
```

A judgment belongs to the Wasm Driver when it would vanish with preview2, or when it is a check a syscall entry makes itself. Every other judgment belongs to `kobako-runtime`.

## Three ABIs

The Guest Binary crosses three ABIs. `wasm-tools print` shows the same 98 imports in all five variants.

| ABI | Imports | Answered by |
|---|---|---|
| kobako ABI | `env.__kobako_upcall` | `kobako-runtime`'s `endpoint` |
| WASI file calls | 21 `fd_*` and `path_*`, and `poll_oneoff` | kobako |
| WASI rest | `environ_*`, `random_get`, `proc_exit` | wasmtime-wasi |
| WASI 0.2 | 71 `wasi:*`, for sockets, HTTP, and their waits | kobako |

The Wasm Driver only wires the kobako ABI's import to `endpoint`.

kobako answers 36 of the 46 preview1 imports: every `fd_*`, `path_*`, and `sock_*` call, and `poll_oneoff`. Those are the calls that read the descriptor table, so the guest stays on one table where each Mount is a preopen. The 14 the Guest Binary never imports serve a third-party guest. The other ten read no descriptor and stay wasmtime-wasi's.

## Syscall Table

wiggle generates the 36 calls from the witx in `crates/kobako-wasmtime/witx/`, as it generates wasmtime-wasi's.

| Property | Consequence |
|---|---|
| witx lists only 36 calls | the rest stay wasmtime-wasi's |
| Generated as wasmtime-wasi's | same argument and answer handling |
| wiggle needs `wasmtime-environ` | table lives in the Wasm Driver |

A value outside what the witx defines traps before a call starts. Since wiggle depends on `wasmtime-environ`, the table cannot enter `kobako-runtime`, which stays free of any Engine.

## Baseline Order

Each preview1 behavior is decided by these sources in order; an earlier one wins.

1. Take the ABI shape and types from the WASI witx.
2. Take each case wasi-testsuite tests from wasi-testsuite.
3. Take semantics neither defines from Linux.
4. Consult wasmtime-wasi as a reference, never as a baseline.

Taking semantics from Linux never departs from POSIX, so a divergence stays a subset: kobako may refuse more, never succeed differently.

## Runtime Decisions

These judgments decide an answer, so `kobako-runtime` holds them whichever ABI asks. Each names its location and the Linux precedent behind it.

| Judgment | Location | Precedent |
|---|---|---|
| Position, append, access, offset limit | `vfs::File` | `struct file` |
| Listing lock and position | `File::iterate_dir` | `iterate_dir` |
| `.` and `..` | Filesystem: `dir_emit_dots` or `Rdir` | libfs, 9p, FUSE |
| Posture mask | `Profile::guest_kstat` | kobako's Posture |
| Read-only, set times, link | `vfs` | Linux VFS |
| Sync and nonblock accepted | `vfs::File` | Linux accepts both |
| Start is not a directory | `set_nameidata` | `set_nameidata` |
| Standard streams, Channel | `filesystem::pipe` | pipefs |
| Path length, NUL | `getname` | `getname` |
| Neither-read-nor-write open | `build_open_flags`, `do_o_path` | `O_PATH` |

The Posture mask hides times, inode numbers, and a link's size; `kobako-runtime` decides it and the Wasm Driver only calls it. An open asking neither to read nor to write stands for its path alone, so it neither asks the Filesystem nor opens a file.

## Driver Decisions

These judgments encode preview1 or repeat a syscall entry's checks, so the Wasm Driver holds them. They group by theme.

| Theme | Rows |
|---|---|
| Descriptor Table | 8 |
| Directory Listing | 4 |
| Error Order | 6 |
| Formats and Waits | 4 |

### Descriptor Table

The table, its numbers, and its rights exist only in preview1, so they stay in the Wasm Driver.

| Judgment | Precedent |
|---|---|
| Rights and prestat | witx encoding |
| Descriptor numbers | `get_unused_fd_flags` |
| Renumber places, then closes | `do_dup2`; witx "atomically replace" |
| Lookup through `fdget` or `fdget_raw` | syscall entry |
| Rights decoded to read and write | wasi-libc, wasi-testsuite |
| Rights a directory passes down | open mode from flags only |
| `fd_fdstat_set_rights` refused | wasmtime-wasi |
| Start checked before a walk | gVisor `revalidatePath` |

Every call finds its descriptor as its Linux syscall does, `sock_*` and `poll_oneoff` included; `kobako-runtime` marks a path-only file as `FMODE_PATH`. Renumbering ignores how the replaced descriptor's close answers. `fd_fdstat_set_rights` answers BADF for a stream and NOTSUP otherwise, since rights only narrow at open and preview2 has none. The start check is kobako's own guarantee, since Linux asks nothing of a walk's starting dentry. The Wasm Driver asks `WalkGuard` wherever a descriptor becomes a walk's start.

### Directory Listing

A preview1 listing writes `dirent` records into a guest buffer, so its layout follows the syscall layer.

| Judgment | Precedent |
|---|---|
| Cookie | witx encoding |
| Each `d_next` filled in after | `filldir64`, `getdents64` |
| Name check, error order | `filldir64` |
| Directory seek refused | wasi-testsuite |

Each entry's name passes `verify_dirent_name` before it is written. Linux would seek a directory, but wasi-testsuite has one refuse it.

### Error Order

Which error answers first is fixed at a Linux syscall entry, before the VFS, so the Wasm Driver keeps that order.

| Judgment | Precedent |
|---|---|
| `fd_filestat_set_size`: size before descriptor | `ksys_ftruncate` |
| `path_open`: flags, EMFILE, then name | `do_sys_openat2`, `FD_ADD` |
| `path_readlink`: empty buffer EINVAL, truncation | `do_readlinkat` `bufsiz` |
| No times set answers 0 first | `SYSCALL_DEFINE4(utimensat)` |
| EAGAIN retried, at most 4 tries | cap-std callers |
| `Escape` answers NOTCAPABLE | witx capability |

A full table answers EMFILE whatever the path names. A readlink buffer is a preview1 shape that preview2 lacks. `kobako-runtime` reports a path escaping its descriptor as `Escape`; Linux would answer EXDEV, but preview1 names a capability the guest does not hold.

### Formats and Waits

The remaining judgments shape what a call writes or how long it waits.

| Judgment | Precedent |
|---|---|
| Argument and `filestat` formats | witx encoding |
| Absolute link target refused | wasi-testsuite |
| Waiting on a pipe's other end: BADF | preview1 only |
| `poll_oneoff` clock wait | Wasm Driver deadline |

`kobako-runtime` reports each error by POSIX name through `FilesystemError::posix_name`, and the Wasm Driver turns the name into a WASI code. A pipe's end is never ready for what the other end does, so a wait on it answers BADF rather than never ending. A clock wait ends at the invocation's deadline, a Wasm mechanism the Wasm Driver owns.

## Call Mapping

Each WASI call reaches the VFS entry its Linux syscall reaches. kobako keeps the Linux name, except that an entry on an open file drops its prefix and hangs off `File`.

| WASI | Linux syscall | Linux VFS | kobako |
|---|---|---|---|
| `path_open` | `openat` | `do_file_open` | same |
| `path_create_directory` | `mkdirat` | `filename_mkdirat` | same |
| `path_unlink_file` | `unlinkat` | `filename_unlinkat` | same |
| `path_rename` | `renameat` | `filename_renameat2` | same |
| `path_filestat_get` | `fstatat` | `vfs_fstatat` | same |
| `fd_read` | `read` | `vfs_read` | `File::read` |
| `fd_readdir` | `getdents64` | `iterate_dir` | `File::iterate_dir` |
| `fd_filestat_set_size` | `ftruncate` | `do_ftruncate` | `File::truncate` |

## WASI Subset

kobako diverges from WASI only by refusing. These refusals are the whole of it.

| Refused | Answer | Owner |
|---|---|---|
| Making a symlink or link | not offered | the built-in Filesystems |
| Reading where a link points | NOTSUP | kobako's rules |
| Opening a FIFO, a socket, or a device | ACCES | kobako's rules |
| `fd_fdstat_set_rights` | NOTSUP | Wasm Driver |

wasi-testsuite at the pinned commit fails only six tests, all for links. Setting times and `fd_allocate` follow Linux, and nonblock is accepted as Linux accepts it. `testsuite-expected.txt` beside the harness names each failing test and why.

## Testsuite Harness

The harness is a Rust test, not wasi-testsuite's Python runner. It stays out of the release gate, which reaches no network.

| Item | Value |
|---|---|
| Test | `preview1/testsuite.rs` in `kobako-wasmtime` |
| Root | a Bind at `/`, one copy per test |
| Source | `WebAssembly/wasi-testsuite`, pinned commit |
| Run | `rake crates:testsuite` |

The task fetches the prebuilt preview1 tests at the commit `tasks/crates.rake` pins, then runs the ignored test in an optimized build. Only the calls kobako deliberately refuses may fail.
