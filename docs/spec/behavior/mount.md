# Mount

How a Host App hands the guest a Mount. A Filesystem mounts at a guest path, seals with the Sandbox, and is replaceable for one invocation.

```
sandbox.mount(at, filesystem, access) ─ sealed at the first invocation
ctx.mount(at, ...)                    ─ replaces it for that invocation only
each invocation ─ Grants ─> runtime ─ preopen ─> guest File
```

### Why these scenarios

A mount is a registration, like a bind, so it follows the same rules. It is refused once the Sandbox is sealed, a path means one mount, and a path never sits inside another. A Context replaces a mount but never adds one.

A mount's block decides in the access's place, once per open or change. Its refusal reaches the guest as a denied call and leaves the run going. A block that raises refuses the same way.

```
block answers truthy  -> the call proceeds
block answers falsy   -> Errno::EACCES in the guest, the script goes on
block raises          -> Errno::EACCES in the guest, the script goes on
```

## Includes

- `test/e2e/test_mount.rb`
- `crates/kobako/tests/mount.rs`

## `MT-001` A mounted Filesystem is reachable from the guest

| Attribute | Value |
| --- | --- |
| relates | `FL-001`, `RT-075` |

| Step | Statement |
| --- | --- |
| Given | a Sandbox with a Memory holding a file mounted at `/data` |
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

## `MT-008` A mount's block that raises refuses the open

| Attribute | Value |
| --- | --- |
| refines | `MT-007` |

| Step | Statement |
| --- | --- |
| Given | a Sandbox whose mount's block raises |
| When | the script opens a path beneath the mount and rescues the failure |
| Then | it rescues `Errno::EACCES` |

## `MT-009` A Context replaces a mount for its invocation only

| Attribute | Value |
| --- | --- |
| relates | `SV-024` |

| Step | Statement |
| --- | --- |
| Given | a Sandbox with a Memory mounted at `/data`, and an invocation whose Context mounted another Memory there |
| When | the next invocation reads beneath `/data` |
| Then | it reads the Sandbox's own Memory |

## `MT-010` A Context cannot mount a path the Sandbox never mounted

| Attribute | Value |
| --- | --- |
| relates | `MT-009` |

| Step | Statement |
| --- | --- |
| Given | a Sandbox with nothing mounted at `/other` |
| When | an invocation's Context mounts a Filesystem at `/other` |
| Then | the override is refused before the guest runs |

## `MT-011` An Overlay built Layer by Layer reaches the guest as one view

| Attribute | Value |
| --- | --- |
| relates | `MT-001`, `FS-008`, `FS-009` |

| Step | Statement |
| --- | --- |
| Given | a Sandbox mounting an Overlay of a writable Temporary over a read-only Memory holding a file |
| When | the script overwrites that file and reads it back |
| Then | it reads what it wrote, and the Memory still holds the file as it was |

## `MT-012` Adding a Layer leaves the Overlay it was added to as it was

| Attribute | Value |
| --- | --- |
| relates | `MT-011` |

| Step | Statement |
| --- | --- |
| Given | an Overlay with one Layer |
| When | a second Layer is added to it |
| Then | the guest reaches only the first Layer through a mount of the original Overlay |
