# Filesystem

The Filesystem Context answers every file call a guest makes beneath a Mount. It is for maintainers changing that answer. The whole map is in [`architecture.md`](../architecture.md); what a Host App relies on is in [`guides/mounts.md`](../guides/mounts.md).

| Document | Holds |
|---|---|
| this one | the layers, their roles, and how they align to Linux |
| [`preview1.md`](preview1.md) | how the Wasm Driver encodes WASI preview1 onto the VFS |
| [`isolation/README.md`](../isolation/README.md) | the Engine and the Wasm Driver's place beside it |

## Three Layers

The Context follows Linux's three layers: the Wasm Driver in the syscall's place, the VFS, and the Filesystems. The security hooks and a Filesystem's operations are hooks the VFS calls at fixed points, not layers of their own.

```
+-------------------------------------------+
| Wasm Driver (a)                           |
+---------------------+---------------------+
                      | calls
                      v
+-------------------------------------------+
| VFS (b)                                   |
|   +-------------+      +-------------+    |
|   | hook: cred  |      | hook: i_op  |    |
|   +------+------+      +------+------+    |
+----------+--------------------+-----------+
           v                    v
+---------------------+ +---------------------+
| Security hooks (c)  | | Filesystem (d)      |
+---------------------+ +---------------------+

+-------------------------------------------+
| locking, sched (e)                        |
+-------------------------------------------+
```

- (a) `kobako-wasmtime`: decoding, errno, the fd table, rights, the Cred, syscall-entry checks.
- (b) `kobako_runtime::vfs`: namei, dentries, inodes, vfsmounts and the mount namespace, open files, the dentry cache.
- (c) In order: kobako's rules, which hold whatever a Mount allows; the modules in `kobako_wasmtime::Config::security`; each Mount's Policy.
- (d) kobako ships Bind, Tmpfs, and Overlay. Ruby lets a Host App write its own in Ruby; pipe serves the standard streams, behind no Mount.
- (e) Beneath the VFS and every Filesystem: `rw_semaphore`, `seqlock`, `current`, and `schedule`, where a Frontend registers its way to wait.

Whoever builds the Wasm Driver hands it the modules, as Linux stacks the LSMs it booted with.

### Module Names

The Filesystems are built on the VFS, so the operation tables belong to `vfs`. The `filesystem` module exports them for a third party bringing its own, and exports `vfs::SuperBlock` as `filesystem::Filesystem`.

`WalkGuard` keeps a Policy judging the path a descriptor stands at. Linux's VFS asks nothing of where a walk starts, so the Wasm Driver holds one in its descriptor table and asks it at syscall entry.

| Module | Holds |
|---|---|
| `kobako_runtime::vfs` | the VFS, the operation tables |
| `kobako_runtime::cred` | the Cred a call is made as |
| `kobako_runtime::filesystem` | the shipped Filesystems, the tables re-exported |
| `kobako_runtime::security` | the hooks, kobako's rules, each Mount's Policy, `WalkGuard` |

## Role Ownership

Each role keeps to its own work, so a change that crosses roles is a change in the wrong place. Only the Wasm Driver, a hook, and what a Frontend registers hold kobako's own design, so the VFS's names follow Linux's.

| Role | Owns |
|---|---|
| Wasm Driver | decoding, errno, fd table, rights, syscall-entry checks, the order a Cred is set up in |
| VFS | the walk, dentries, inodes, open files, mounts, operation tables, what an entry lacks |
| Security hooks | whether a path may be reached, `WalkGuard`, the Cred's restriction |
| Filesystem | what one entry answers, its own state on a dentry or inode |
| locking, sched | whether a wait can end, who asks, the Frontend's way to wait |

The Driver sets a Cred up as a Landlock sandboxer does: it opens the guest's root, then restricts the Cred through the runtime. What the Cred may reach is judged only by the hooks.

The VFS also charges what a Cred's Filesystems hold to that Cred's memory budget, the one the guest's own growth is charged to.

## Linux Alignment

WASI governs the boundary and Linux governs the inside. WASI fixes which calls exist and their shape, and leaves the meaning to POSIX; kobako fills that meaning from Linux.

```
+--- WASI ------------------------------+
|  which calls (a)                      |
+------------------+--------------------+
                   |
                   | "similar to POSIX"
                   v
+--- Linux -----------------------------+
|  each answer (b)                      |
+---------------------------------------+
```

- (a) Which calls exist, their argument shapes, and the errno codes.
- (b) How each answer is decided: the VFS, the LSM hooks, and `f_op`.

## WASI and POSIX

WASI preview1 is an ABI shaped like POSIX. It fixes the shape, not the errno for each case, so the answers below its boundary need a baseline of their own.

| Fact | Source |
|---|---|
| functions and errno codes align to POSIX | the wasi.dev preview1 page |
| most functions say "similar to" a POSIX call | the preview1 witx |
| a new fd need not be the lowest free one | the preview1 witx, `path_open` |
| implementations answer differently | Go issue 59583 |
| wasi-testsuite accepts any errno by default | its `config.rs`, `Permissive` |

## One Call

One WASI call passes the Wasm Driver, the VFS, and a Filesystem in turn, and each meets one layer of Linux. The Wasm Driver follows WASI; the two layers below follow Linux.

```
  guest     path_open (a)
    |
    v
+--- Wasm Driver -----------------------+
|  decode OpenHow                       |   ~ openat(2)
+------------------+--------------------+
                   v
+--- VFS -------------------------------+
|  open path (b)                        |   ~ do_file_open
+------------------+--------------------+
                   v
+--- Filesystem ------------------------+
|  InodeOperations::open                |   ~ ->open
+---------------------------------------+
```

- (a) For example `path_open(dir, "a/b", ...)`.
- (b) It walks the path, asks `may_open`, and asks the security hooks, as Linux asks each LSM.

## Divergence Kinds

kobako differs from Linux in four kinds of way. As to which call succeeds, all four only refuse more; none lets a call succeed that Linux would refuse. What a successful stat reports, such as a link's size or an inode number, is the Profile's to mask.

| Kind | Example | Owner |
|---|---|---|
| stricter, stacked on Linux | refusing a FIFO open or `readlink` | kobako's rules in `security` |
| a subset, refusing a Linux feature | making a link | the shipped Filesystems ([`preview1.md`](preview1.md)) |
| a Linux layer not adopted | the fd table, processes | the Wasm Driver |
| mounts placed again each run | a move above a mount, a link on its path | the VFS's mount namespace |

### Re-placement

Each Invocation builds its mount namespace again and places every Mount by its path, as an Invocation keeps no state. Linux's namespace persists, so a mount follows its dentry wherever a directory moves.

```
Linux:  namespace persists ---> follows dentry (a)
kobako: placed per run -------> path held (b) --+--> EBUSY (c)
                                                +--> ELOOP (d)
```

- (a) A mount moves with the directory it stands on.
- (b) The names a mount's path leads through must still lead there next run.
- (c) A directory above a mount neither moves nor goes.
- (d) A mount's path passes no link, which another writer could change.

Each departure of this kind keeps a mount's path stable between runs. A new one traces back here, or it is a mistake.

## Lock Order

The VFS takes its sleeping locks in Linux's order, ranked as `directory-locking.rst` ranks them. This sets the vfs, dcache, and fsi lock groups' own order; [`locking/`](../locking/README.md) holds the edges between groups.

```
+------------------+
| rename_mutex (a) |
+------------------+
         | then
         v
+------------------+
| ancestor dir (b) |
+------------------+
         | then
         v
+------------------+
| other dir        |
+------------------+
         | then
         v
+------------------+
| non-dirs (c)     |
+------------------+
```

- (a) One for each Filesystem, taken only where a move spans two directories, as `lock_rename`.
- (b) The directory above the other first, as `lock_two_directories`.
- (c) A directory a move takes along first, then the rest by address, as `lock_two_nondirectories`.

A call taking one directory and the entry in it takes a step of this chain.

### Stacked Filesystems

A Filesystem stacked on another ranks below it, so an Overlay's locks all come before its Layers'.

| Rank | Locks |
|---|---|
| 1 | the Overlay's own chain |
| 2 | an Overlay entry's copy-up lock |
| 3 | each Layer's own chain |

A Layer never takes an Overlay's lock. A run started inside a Ruby Filesystem's callback keeps this order only where its Mounts reach Filesystems ranked after that one. A stacked Filesystem likewise calls only into higher ranks.

### Dentry Cache

The dentry cache's locks are leaves beneath the chain, taken for a short step.

```
inode dentry -> dentry hashtable -> RENAME_LOCK -> dentry state -> dentry LRU
```

`RENAME_LOCK` is never taken again inside its own write, since its writer's lock is no re-entrant one. Two dentries' states are held at once only inside that write, as `d_exchange` holds them, so their order never matters. `__d_unalias` alone takes a sleeping lock beneath it, and only by trylock, answering ESTALE where another call holds it.

### Filesystem State

A Filesystem's own state is a leaf beneath the chain, except where one entry's state holds another's.

| Filesystem | Order |
|---|---|
| Tmpfs | a directory before an entry it holds, so its locks follow its tree |
| the rest | one lock at a time |
