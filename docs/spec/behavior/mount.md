# Mount

How a Host App hands the guest a Mount. A Filesystem mounts at a guest path, seals with the Sandbox, and is replaceable for one invocation.

```
sandbox.mount(at, filesystem, access) ─ sealed at the first invocation
ctx.mount(at, ...)                    ─ replaces it for that invocation only
each invocation ─ Grants ─> runtime ─ preopen ─> guest File
```

### Why these scenarios

A mount is a registration, like a bind, so it follows the same rules. It is refused once the Sandbox is sealed, a path means one mount, and a path never sits inside another. A Context replaces a mount but never adds one.

A mount's block decides in the access's place, at most once per open or change. Its refusal reaches the guest as a denied call and leaves the run going. A block raising a StandardError refuses the same way. Anything else it raises belongs to the host.

```
block answers truthy        -> the call proceeds
block answers falsy         -> Errno::EACCES in the guest, the script goes on
block raises StandardError  -> Errno::EACCES in the guest, the script goes on
block raises anything else  -> the run ends, and it raises in the Host App
```

## Includes

- `test/e2e/test_mount.rb`
- `test/e2e/test_host_exception.rb`
- `test/e2e/test_mount_limit.rb`
- `test/e2e/test_mount_rename.rb`
- `test/e2e/test_overlay_threads.rb`
- `test/e2e/test_directory_lock_threads.rb`
- `crates/kobako/tests/integration/mount.rs`
- `test/parity/test_mount.rb`

## `MT-001` A mounted Filesystem is reachable from the guest

| Attribute | Value |
| --- | --- |
| relates | `FL-001`, `RT-075` |

| Step | Statement |
| --- | --- |
| Given | a Sandbox with a Tmpfs holding a file mounted at `/data` |
| When | the script reads that file beneath `/data` |
| Then | it reads the file's content |

## `MT-002` A mount after the first invocation is refused

| Attribute | Value |
| --- | --- |
| relates | `S-018` |

| Step | Statement |
| --- | --- |
| Given | a Sandbox that has run an invocation |
| When | a Filesystem is mounted on it |
| Then | the mount is refused |

## `MT-003` A mount path that is not an absolute guest path is refused

| Step | Statement |
| --- | --- |
| Given | a Sandbox |
| When | a Filesystem is mounted at a relative path or one climbing with `..` |
| Then | the mount is refused |

## `MT-004` A path mounted twice is refused

| Attribute | Value |
| --- | --- |
| relates | `MT-003` |

| Step | Statement |
| --- | --- |
| Given | a Sandbox with a Filesystem mounted at `/data` |
| When | another Filesystem is mounted at `/data` |
| Then | the mount is refused |

## `MT-005` A mount nested inside another is refused

| Attribute | Value |
| --- | --- |
| relates | `MT-004` |

| Step | Statement |
| --- | --- |
| Given | a Sandbox with a Filesystem mounted at `/data` |
| When | another Filesystem is mounted at `/data/inner` |
| Then | the mount is refused |

## `MT-006` A runtime presenting no Mount refuses the mount

| Attribute | Value |
| --- | --- |
| relates | `RT-075` |

| Step | Statement |
| --- | --- |
| Given | a Sandbox over a runtime that supports no Mount |
| When | a Filesystem is mounted on it |
| Then | the mount is refused as a setup failure |

## `MT-007` A mount's block refusing an open denies it and the script goes on

| Attribute | Value |
| --- | --- |
| refines | `RT-080` |

| Step | Statement |
| --- | --- |
| Given | a Sandbox whose mount's block refuses a path |
| When | the script opens that path and rescues the failure |
| Then | it rescues `Errno::EACCES` and runs to its end |

## `MT-008` A mount's block that raises a StandardError refuses the open

| Attribute | Value |
| --- | --- |
| refines | `MT-007` |

| Step | Statement |
| --- | --- |
| Given | a Sandbox whose mount's block raises a `RuntimeError` |
| When | the script opens a path beneath the mount and rescues the failure |
| Then | it rescues `Errno::EACCES` |

## `MT-009` A Context replaces a mount for its invocation only

| Attribute | Value |
| --- | --- |
| relates | `SV-024` |

| Step | Statement |
| --- | --- |
| Given | a Sandbox with a Tmpfs mounted at `/data`, and an invocation whose Context mounted another Tmpfs there |
| When | the next invocation reads beneath `/data` |
| Then | it reads the Sandbox's own Tmpfs |

## `MT-010` A Context cannot mount a path the Sandbox never mounted

| Attribute | Value |
| --- | --- |
| relates | `MT-009` |

| Step | Statement |
| --- | --- |
| Given | a Sandbox with nothing mounted at `/other` |
| When | an invocation's Context mounts a Filesystem at `/other` |
| Then | the override is refused before the guest runs |

## `MT-011` An Overlay reaches the guest as one view

| Attribute | Value |
| --- | --- |
| relates | `MT-001`, `FS-008`, `FS-009` |

| Step | Statement |
| --- | --- |
| Given | a Sandbox mounting an Overlay of an upper Tmpfs over a lower Tmpfs holding a file |
| When | the script overwrites that file and reads it back |
| Then | it reads what it wrote, and the lower Tmpfs still holds the file as it was |

## `MT-013` A Tmpfs's size given in Ruby bounds what the guest writes

| Attribute | Value |
| --- | --- |
| refines | `FL-012` |

| Step | Statement |
| --- | --- |
| Given | a Sandbox mounting a Tmpfs seeded with files and given a size |
| When | the script writes past that size |
| Then | the script raises `Errno::ENOSPC`, and the seeded files still read |

## `MT-015` A Tmpfs refuses a keyword it does not know

| Attribute | Value |
| --- | --- |
| relates | `MT-013` |

| Step | Statement |
| --- | --- |
| Given | no Tmpfs |
| When | one is created with a misspelled `size` keyword |
| Then | the creation raises `ArgumentError` naming the unknown keyword |

## `MT-016` A mount's block answers after the host compacts its heap

| Attribute | Value |
| --- | --- |
| relates | `MT-007`, `T-071` |

| Step | Statement |
| --- | --- |
| Given | a Sandbox whose mount's block allows a path |
| When | the host compacts its heap between two opens of that path |
| Then | the second open reads the file as the first did |

## `MT-017` Inodes given in Ruby bound what the guest creates

| Attribute | Value |
| --- | --- |
| refines | `FS-030` |
| relates | `MT-013` |

| Step | Statement |
| --- | --- |
| Given | a Sandbox mounting a Tmpfs given a count of inodes |
| When | the script creates empty files past that count |
| Then | the script raises `Errno::ENOSPC` |

## `MT-018` A directory beneath a mount with a block does not move

| Attribute | Value |
| --- | --- |
| refines | `MT-007` |
| relates | `RT-094` |

| Step | Statement |
| --- | --- |
| Given | a Sandbox whose mount's block refuses one file inside a directory |
| When | the script renames that directory and reads the file beneath the new name |
| Then | the rename raises `Errno::EXDEV`, and the file is not there |

## `MT-019` Sandboxes on several Threads share an Overlay with a Ruby Layer

| Attribute | Value |
| --- | --- |
| refines | `FS-039` |

| Step | Statement |
| --- | --- |
| Given | two Threads, each with a Sandbox mounting one Overlay of an upper Tmpfs over a Ruby Layer |
| When | both evaluate scripts reaching the Overlay at once, whichever GVL mode each takes |
| Then | both finish |

## `MT-020` A Sandbox reaching an Overlay from inside its own call raises EDEADLK

| Attribute | Value |
| --- | --- |
| refines | `FS-038` |

| Step | Statement |
| --- | --- |
| Given | an Overlay whose lower Ruby Layer evaluates a script through that Overlay |
| When | the inner script reads a file there |
| Then | it raises `Errno::EDEADLK` |

## `MT-021` A Filesystem a Rust Host App writes mounts like the kinds kobako ships

| Attribute | Value |
| --- | --- |
| relates | `MT-001`, `RF-001` |

| Step | Statement |
| --- | --- |
| Given | a Filesystem a Host App writes against the Rust SDK alone |
| When | it is mounted and the script reads a file it serves |
| Then | the script reads that file's content |

## `MT-022` What is no Filesystem is refused as the wrong type

| Attribute | Value |
| --- | --- |
| relates | `MT-003`, `MT-011` |

| Step | Statement |
| --- | --- |
| Given | an object that is no kind kobako ships and answers neither `open` nor `stat` |
| When | it is mounted on a Sandbox, or added to an Overlay as a Layer |
| Then | each raises `TypeError` |

## `MT-023` What a mount's block raises beyond StandardError ends the run

| Attribute | Value |
| --- | --- |
| relates | `MT-008`, `RF-014`, `T-282` |

| Step | Statement |
| --- | --- |
| Given | a Sandbox whose mount's block raises `Interrupt` |
| When | the script opens a path beneath the mount and rescues every failure |
| Then | the run raises `Interrupt` in the Host App |

## `MT-024` A Tmpfs given files it cannot hold raises ArgumentError

| Attribute | Value |
| --- | --- |
| relates | `FS-062`, `FS-067`, `MT-015` |

| Step | Statement |
| --- | --- |
| Given | files naming the root, or a file where another path needs a directory |
| When | a Tmpfs is created with them |
| Then | the creation raises `ArgumentError` naming the path |

## `MT-025` A mount's block is asked once for a read, as a read

| Attribute | Value |
| --- | --- |
| refines | `MT-007` |

| Step | Statement |
| --- | --- |
| Given | a Sandbox whose mount's block records each access it is asked |
| When | the script reads a file beneath the mount |
| Then | the block was asked once, for `:read` |

## `MT-026` A Sandbox waiting on a directory another Thread changes lets the change finish

| Attribute | Value |
| --- | --- |
| refines | `RT-166` |
| relates | `MT-019` |

| Step | Statement |
| --- | --- |
| Given | one Overlay over a Ruby Layer that sleeps while asked, shared by two Threads |
| When | one Sandbox creates a file while the other, holding the GVL, looks in that directory |
| Then | both finish |
