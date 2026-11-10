# Isolation

The Isolation Context owns the Engine, the Wasm Driver that drives it, and the Ambient authority a Posture grants. This document records where the Wasm Driver sits, what it may decide, and when that placement should be reconsidered. The whole map is in [`architecture.md`](../architecture.md).

| Document | Holds |
|---|---|
| this one | the Wasm Driver's role, its replacement layer, the Engines, standard bridging |
| [`architecture.md`](../architecture.md) | the parts, the seams, capability negotiation |
| [`filesystem/README.md`](../filesystem/README.md) | the VFS beneath the Wasm Driver |
| [`filesystem/preview1.md`](../filesystem/preview1.md) | every preview1 judgment, classified |
| [`guides/customization.md`](../guides/customization.md) | bringing another Engine |

## Name Map

An Engine and the Wasm Driver are two concepts: one runs wasm, the other drives it. Each concept has its own name in code, and the two differ. `Runtime` the trait and `kobako-runtime` the crate are also two things.

| Name | Means | In code |
|---|---|---|
| Engine | the wasm runtime itself | wasmtime, WasmEdge |
| Wasm Driver | the layer that drives an Engine | `kobako_wasmtime::Driver` |
| `Runtime` | the trait each Wasm Driver implements | `kobako_runtime::Runtime` |
| `kobako-runtime` | the crate that decides every answer | the `kobako-runtime` crate |
| Filesystem | what answers for entries beneath a Mount | Tmpfs, Bind, Overlay |

The wasmtime type is `Driver` with no prefix, because its crate path already names the Engine. A Filesystem is never called a driver, so the word stays with the Wasm Driver.

## Abstraction Map

kobako's layers line up with gVisor's and Linux's. The table is an analogy only; the names above stay the formal ones.

| Role | kobako | gVisor | Linux |
|---|---|---|---|
| program | Guest Binary | application | process |
| libc | wasi-libc | libc | glibc |
| boundary | WASI preview1, kobako ABI | syscall ABI | syscall ABI |
| decides answers | `kobako-runtime` | Sentry | kernel VFS |
| encodes | Wasm Driver | syscall table | syscall entry |
| executes | Engine | Platform | CPU |

The rows run in the order one system call passes through them. The boundary row is where the guest ends and the host begins.

## Driver Role

`kobako-runtime` decides the answers, and the Wasm Driver only encodes. A judgment belongs to the Wasm Driver in two cases. Either it would vanish under preview2, or it is a check a syscall entry makes itself, such as which descriptor a call takes. Every other judgment belongs to `kobako-runtime`. Reading a clock, the environment, and random bytes are answered by native WASI instead, with the frozen sources wired in by the Wasm Driver. A clock's `subscribe-duration` is kobako's, since its pollable waits beside a socket's.

```
+- Wasm Driver: encode ----------------+
|   decode arguments                   |
|   errno codes                        |
|   fd table                           |
|   rights, prestat                    |
|   syscall-entry checks, Cred         |
|   frozen ambient sources             |
+------------------+-------------------+
                   | asks
                   v
+- kobako-runtime: decide -------------+
|   Open File                          |
|   VFS, Policy hook                   |
|   Filesystems                        |
+--------------------------------------+
```

Errors follow the same split. `kobako-runtime` names each failure in POSIX terms through `FilesystemError::posix_name`, and the Wasm Driver turns that name into a WASI code. Where each preview1 judgment lands is in [`filesystem/preview1.md`](../filesystem/preview1.md).

## Kernel Assembly

The kernel is the whole the Wasm Driver assembles from `kobako-runtime`'s subsystems. Besides encoding, the Wasm Driver chooses which security modules to stack, as Linux picks its LSMs with `lsm=` at boot.

| Linux | kobako | Does |
|---|---|---|
| Kconfig, boot `lsm=` | Wasm Driver and its `Config` | assembles |
| syscall entry | Wasm Driver | encodes |
| task's `cred`, `files_struct` | Wasm Driver's Cred, fd table | holds per caller |
| `fs/`, `security/` | `kobako-runtime` | decides answers |
| CPU | Engine | executes |

Modules arrive through `kobako_wasmtime::Config::security`. Each is asked after kobako's own rules and before any Mount's Policy. Assembly only picks subsystems and decides no single call's answer, so the Driver Role test still holds.

## Replacement Layers

The WASI ecosystem has eight layers where a filesystem can be replaced. The upper four run inside the guest's trust domain, so none of them can hold a security boundary.

```
+- guest trust domain ----------------+
|   L1  libc           wasi-libc      |
|   L2  in-guest VFS   wasi-vfs       |
|   L3  transform      wasm-tools     |
|   L4  composition    WASI-Virt      |
+-------------------------------------+

====  L5  core-module imports  ====  <-- kobako
====  L6  WIT interfaces       ====

+- host trust domain -----------------+
|   L7  backend trait  virtual_fs     |
|   L8  WASI library   wasi-common    |
+-------------------------------------+
```

kobako replaces the filesystem imports at L5. Every Engine compared can register imports there.

| Layer | Engines | For kobako |
|---|---|---|
| L1-L4 | all | can only add on top |
| L5 | every Engine compared | the current place |
| L6 | wasmtime, jco | needs a component guest; boundary cost |
| L7 | wasmer, wazero | path interface tied to one Engine |
| L8 | wasmi | its underlying crate is frozen |

L7 has no hook in wasmtime. `wasmtime-wasi` 50's `WasiFilesystemCtx` holds only host directory preopens confined by `cap-primitives`, with no backend trait. `wasi-common` was removed in wasmtime 47.

## Engine Interfaces

A Wasm Driver for each Engine builds on what that Engine offers for host functions. The table records the current releases, checked on 2026-11-06.

| Engine | Registers | Shares preview1 names | Freezes clock |
|---|---|---|---|
| wasmtime 50 | `func_new` | shadowing | yes |
| wasmi 2.0 | `func_new` | shadowing | yes |
| wasmer 7.5 | `new_with_env` | overrides | no |
| WasmEdge 0.18.1 | `fn` pointer | no | no |
| WAMR 2.4 | Rust SDK `wamr-rust-sdk` 1.1 | global registration | no |

Shadowing lets kobako answer its calls and leave the rest to the Engine's native WASI. WAMR's Rust SDK still registers natives once for the whole process, at runtime init. WasmEdge lets only one provider own a module name, so its Wasm Driver would answer all 46 preview1 imports itself. An Engine that cannot freeze its clock declares the permissive Posture, as [`architecture.md`](../architecture.md) § Capability Negotiation sets out.

## Native Delegation

The same test applies beyond the filesystem. kobako answers only what native WASI cannot give and kobako's guarantees need.

| Concern | Answered by | Reason |
|---|---|---|
| reading a clock, environment, random | native WASI | the Wasm Driver wires in the frozen sources |
| a clock's `subscribe-duration` | `kobako-runtime` | its pollable waits beside a socket's |
| Preamble, Channel | `kobako-runtime` | kobako's own concepts |
| kobako ABI host side | `kobako-runtime` | no Engine supports it |
| memory cap accounting | `kobako-runtime` | the growth hook stays in the Wasm Driver |
| networking | `kobako-runtime` | a new kind of Grant |

Preview1 has `sock_accept`, `sock_recv`, `sock_send`, and `sock_shutdown`, but no connect. Networking is therefore encoded as WASI 0.2's `wasi:sockets`, `wasi:http`, and `wasi:io`, as imports a core module links.

## Standard Bridging

Each ABI version gets its own syscall table, and every table asks the same `kobako-runtime` for answers. As WASI evolves, kobako adds an entry and rewrites nothing beneath it.

```
          one core module
         |               |
         v               v
+-----------------+  +-------------------+
| p1 fd table     |  | p2 resource table |
| files           |  | sockets, http, io |
+--------+--------+  +---------+---------+
         |                     |
         +----------+----------+
                    v
          +-----------------------+
          | kobako-runtime        |
          | Open File, VFS,       |
          | OpenFiles             |
          +-----------------------+
```

Each ABI's encoding keeps its own table: preview1's fd table and preview2's resource table. Both take numbers from one space, as Linux's one fdtable numbers a file and a socket alike. Both count against one `OpenFiles`, as `RLIMIT_NOFILE` counts every descriptor a process holds. Every table refers to the same Open File, which `kobako-runtime` holds. Precedents take this shape:

| Precedent | Shape |
|---|---|
| `wasmtime-wasi` | preview1 built on preview2 |
| Linux compat syscalls | a second table, one kernel |
| io_uring | the syscalls' VFS entry points (`io_uring/fs.c`) |

### Shared Calls

Where both encodings answer the same call, preview1 adapts to preview2, as `wasmtime-wasi`'s preview1 does. Waiting is that call today. `poll_oneoff` turns each subscription into a preview2 pollable and waits through preview2's poll, so one rule decides when a clock is due. A socket read or write through preview1 still reaches the socket's Open File, since its POSIX error names have no preview2 code.

| Call | preview1 | Answered by |
|---|---|---|
| waiting | `poll_oneoff` | preview2's poll |
| a socket's read, write | `fd_read`, `fd_write` | the socket's Open File |
| a file's call | `path_*`, `fd_*` | the Open File, preview1 only |

## Trigger Conditions

kobako stays at L5 until one of these holds; then L6 or the bridging is reconsidered.

| Condition | Action |
|---|---|
| the guest builds for preview2 | add a `wasi:filesystem` entry |
| `wasmtime-wasi` opens a filesystem backend | weigh moving onto it |

kobako takes preview2 only where preview1 has no call, as networking, and as imports a core module links rather than a component. The filesystem stays on preview1 until a condition holds. Every other Engine speaks preview1, so none of them is a condition.
