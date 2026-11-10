# Mounts

A Mount hands the guest a Filesystem at one guest path. kobako keeps the guest
beneath that path and holds the Mount's access. What the Filesystem holds, and
which paths a Policy allows, is the Host App's choice.

| kobako keeps | The Host App chooses |
|---|---|
| the guest beneath its Mount | which Filesystem to mount |
| the Mount's access | read, or read and write |
| each path a Policy judges | what the Policy allows |
| a Tmpfs within its caps | `size:` and `nr_inodes:` |

The isolation boundary around a Mount is in [Security Model](security.md), and
how the Filesystems are built is in [Filesystem](../filesystem/README.md).

## Filesystem Kinds

`mount` places a Filesystem at an absolute guest path. The default access is
`:read`, so a write beneath it raises `Errno::EROFS` in the guest.

```ruby
sandbox.mount("/config", Kobako::Filesystem::Tmpfs.new("app.yml" => yaml))
sandbox.mount("/data", Kobako::Filesystem::Bind.new("var/data"))
sandbox.mount("/tmp", Kobako::Filesystem::Tmpfs.new, access: :read_write)
sandbox.eval('File.write("/tmp/x", "1")') do |ctx|
  ctx.mount("/tmp", Kobako::Filesystem::Tmpfs.new, access: :read_write)
end
```

| Kind | Holds |
|---|---|
| `Tmpfs.new(files = {}, size:, nr_inodes:)` | files in memory |
| `Bind.new(path)` | a host directory; nothing outside it is reachable |
| `Overlay.new(lower: [fs, ...], upper: tmpfs)` | read-only Layers under a Tmpfs that takes every change |

Every Invocation reaches the same Filesystem, so a run that must start empty
replaces the mount with its own Tmpfs through `ctx.mount`. `ctx.mount` replaces
only a path already mounted. Mounts seal with the Sandbox, and a mount may sit
inside another; the guest reaches the innermost one.

### Tmpfs Bounds

A Tmpfs is bounded as a Linux tmpfs mount is. Past either bound, the guest
raises `Errno::ENOSPC`.

| Bound | Unless given | `nil` |
|---|---|---|
| `size:` | 64 MiB of file bytes | no bound |
| `nr_inodes:` | one entry per 4 KiB of `size`, and the root | no bound |

### Overlays

An Overlay stacks Layers as Linux's overlayfs does. The lower Layers, topmost
first, are never written, and the upper must be a Tmpfs, so the host stays as
it was.

```ruby
overlay = Kobako::Filesystem::Overlay.new(
  lower: [Kobako::Filesystem::Bind.new("app")], # never written
  upper: Kobako::Filesystem::Tmpfs.new          # every change lands here
)
sandbox.mount("/app", overlay, access: :read_write)
```

| Change | Lands as |
|---|---|
| a write to a lower file | a copy up into the upper first |
| a new or renamed file | a file in the upper |
| a removal | a whiteout hiding the lower file, reserving no name |

Without `upper:`, every change raises `Errno::EROFS`, and two lower Layers are
needed. An Overlay takes at most 500 lower Layers and stacks at most two deep.
The upper Tmpfs keeps changes in `upper/` and builds each in `work/` first, the
shape gVisor also requires.

### Ruby Objects

Any object answering `open` and `stat` mounts as a Filesystem, directly or as a
Layer. Its methods follow Ruby's `File`, `Dir`, and `IO`, so a real `::File`
can answer as an open file. `Kobako::Filesystem` lists every method.

| It raises | The guest sees |
|---|---|
| an `Errno` the guest has a class for | that `Errno` |
| any other `StandardError` | `Errno::EIO` |
| nothing, lacking a change method | `Errno::EROFS` for that change |
| nothing, lacking `append` | `Errno::ENOTSUP` for an append |

A path is relative to the mount, the empty path being its root. Every call
re-enters Ruby, and its time counts toward `timeout:` uninterrupted.

## Guest File

The guest's `File.open` takes a mode String such as `"r"`, `"w+"`, or `"wx"`, or
an Integer of `File::Constants` flags valued as CRuby's on Linux. An exclusive
create lets Sandboxes sharing a directory agree on a lock file.

Beneath a mount, `File`, `Dir`, and `IO` answer as CRuby does over a Linux
directory. Where they differ, they only refuse more.

| When | The guest sees |
|---|---|
| nothing is mounted | `NotImplementedError` |
| a Policy refuses a change | `Errno::EACCES`, where a read-only mount says `Errno::EROFS` |
| an Integer mode holds a flag kobako does not honor, or `WRONLY` beside `RDWR` | `Errno::EINVAL` |
| a mode names an encoding, such as `"r:utf-8"` | `ArgumentError` |
| `IO.new` wraps stdout or stderr in a mode other than `"w"` | `ArgumentError` |
| `IO#advise` names a negative offset | `Errno::EINVAL`, where CRuby answers `nil` |
| a directory opened as a `File` seeks or asks its position | `Errno::EISDIR`, as WASI keeps no directory position |
| the critical flag `disable_file` is set | no `File` or `Dir` constant |

### File Reports

`File::Stat` answers as CRuby does on Linux, for what preview1 hands the guest.
What it could only answer wrongly stays undefined, so a script fails rather
than read a wrong answer.

| Part | Holds |
|---|---|
| ways to ask | `File.stat`, `File.lstat`, `File::Stat.new`, `File#stat`, `File#lstat` |
| answers | `ino`, `nlink`, `size`, `size?`, `zero?`, `ftype`, and the kind predicates |
| class calls | `File.ftype`, `File.symlink?`, `File.size?`, `File.zero?`, `File.empty?` |
| undefined | times, `<=>`, `dev`, `mode`, owners, permissions, `pipe?`, `blockdev?`, `File.identical?` |

The kind predicates are `file?`, `directory?`, `symlink?`, `chardev?`, and
`socket?`. A FIFO's and a block device's `ftype` read `"unknown"`, and a
hermetic guest reads every `ino` as 0.

## Mount Choices

A Bind opens only files and directories: a FIFO, socket, or device raises
`Errno::EACCES`. On Windows, so does a spelling the host lists under no name.

| Choose | Because |
|---|---|
| `access: :read` unless the guest must write | a read-only Mount refuses every change |
| a Tmpfs over a Bind | a Bind is a real host path the guest changes |
| `size:` and `nr_inodes:` sized to the task | the defaults hold 64 MiB and 16385 entries |
| `open_file_limit:` sized against the host's own | what the guest opens holds host descriptors |
| no writable grant over kobako's compile cache | the host runs what that cache holds as native code |

Each file the guest holds open holds one host descriptor. Each directory it
holds open holds one for each Bind it spans, so an Overlay's directory holds one
per Layer that is a Bind. A Bind also keeps up to 64
directories open for walks, besides its root.

The compile cache is `$XDG_CACHE_HOME/kobako`, else `~/.cache/kobako`. A Bind of
`$HOME` covers it, and so does a Ruby Filesystem or Service that writes there.

## Mount Policies

A Policy decides each open and each change by path, and a refusal raises
`Errno::EACCES` while the script goes on. Ruby spells it as the block `mount`
takes; the Rust SDK takes it through `mount_with_policy`.

```ruby
sandbox.mount("/data", fs) { |path, _access| path.start_with?("/data/public/") }
```

The block judges only what the access already allows, so it can only refuse
more. A falsy answer or a `StandardError` from it refuses that call.

Allow what the guest needs rather than refusing what it must not reach: a
denylist misses a path you did not foresee.

### Judged Paths

The Policy judges paths, so nothing may reach a path it never judged. This
table shows what the guest sees where a path could slip past it.

| Beneath a Mount with a Policy | The guest sees |
|---|---|
| a rename of a directory | `Errno::EXDEV`, and it copies entry by entry |
| a path through a host link | judged where the link leads, never by the link's own path |
| a link another writer swaps in after the Policy judges | never followed; the call reaches the directory it walked to |
| another case or Unicode form of a name | the Policy judges the spelling the host stores |

A call that what stands at the path refuses, such as `Errno::ENOENT`, fails
before the Policy is asked. An open that may create is asked a change first,
then what it opens for, as Linux asks its security modules. Its
`Errno::EEXIST` comes after the Policy.

## Refused Paths

The Policy does not judge `stat` or a listing, as Landlock leaves `stat(2)`
alone. A refused path keeps its content private and still reports the rest.

| The guest reads | Reveals |
|---|---|
| that the path exists, and its kind | what stands there |
| its size | how long the content is |
| its inode number, on `permissive` | whether two paths are one file |
| its count of names, on either Posture | hard links, including any outside the Mount |

Keep a file out of the Mount when even these must stay private.

## Shared Filesystems

kobako defends a Mount against the guest, and against another Sandbox's guest
writing to the same Filesystem. A change from anywhere else is the host's.

| Writer | kobako's answer |
|---|---|
| the guest itself | kept beneath its Mount, its access held |
| another Sandbox's guest sharing the Filesystem | neither guest reaches what its Mount or Policy refuses |
| a guest of another Mount of the same Filesystem | only that Mount's Policy judges it; a Policy guards its own Mount's paths |
| the host, or a change to a Layer outside its Overlay | no crash or deadlock; the rest is the host's |

A Filesystem keeps names a walk found, so one guest's walks can speed up
another's. The bundled Guest Binaries read no clock, using one only to bound a
wait, so no script can time that. A third-party guest on `permissive`, or a script a Service hands a clock,
may tell which names were reached lately.

## Opened Directories

A directory the guest opened is held as itself, as a POSIX descriptor holds one.
Beyond each Mount's root, only a third-party guest holds a directory open to
walk from.

| After it opens | A call through it |
|---|---|
| it moves | reaches it where it went, as Linux |
| it is removed | makes nothing more |

A path resolved through it stays beneath it, as a descriptor only ever narrows
what the guest may reach.

### Beneath a Policy

The Policy judges a path through the directory where the directory stands now.
How it moved decides whether it is still reached.

| How it moved | A path through it |
|---|---|
| through any Mount of the same Filesystem | reaches it where it went, judged there |
| as a call starts from it, through another Mount | that call and each later one reach it where it went |
| by the host, or through another Filesystem | raises `Errno::ENOENT` |
| on a Filesystem that reports no identity | raises `Errno::ENOENT` |

### Descriptor Reads

The Policy judges neither a `stat` nor a listing of the descriptor itself. This
table shows what the guest still reads once the host moved the directory out of
the Mount.

| Through the descriptor | Answer |
|---|---|
| a `stat` of it | what it reports then |
| a listing of it | the names it holds then, new ones included |
| a path beneath it, content included | `Errno::ENOENT` |

## Host Risks

Some risks stay with the host, because kobako cannot see them. A row marked
third-party needs what the bundled Guest Binaries never do, so only a
third-party guest reaches it. It needs a call they do not import, listed below,
or a walk from a directory the guest opened.

| A guest call | Preview1 imports it lacks |
|---|---|
| set a time | `path_filestat_set_times`, `fd_filestat_set_times` |
| make or read a link | `path_symlink`, `path_link`, `path_readlink` |

### Host Directories

A Bind is a live host directory, and an Overlay's Layers may be too.

| Risk | Where it lies |
|---|---|
| another process changing a Bind's host files | the host directory |
| a Bind's host disk | kobako caps no space there; the host directory's own quota does |
| a Bind's directories stacked past `PATH_MAX` by moves | the host's own tools may refuse or slow on them |
| a time a guest sets on a Bind's entry, third-party | it stands on the host; a hermetic guest still reads the epoch |
| a device swapped into a Bind as the guest opens a file | opened once before its refusal |
| a directory the guest lists in steps | a Tmpfs lists as it stands; others hold entries |
| a directory the host swaps in just as a Policy judges an opened one, third-party | the host's race |

A Filesystem holding a listing's entries raises `Errno::ENOMEM` once they pass
`memory_limit`.

### Host Links

The guest makes no link through the bundled Guest Binaries, but a host link in
a Bind still leads somewhere.

| Risk | Where it lies |
|---|---|
| a hard link or mount point inside a Bind | the guest reaches what it leads to |
| a link swapped in between kobako's check and its open, without a Policy | the host's race, never leading beyond the Bind |
| a link in a Bind | followed within the Bind; its target is never read |
| where such a link leads | told by what the guest finds there |
| a link through a directory the guest opened, third-party | followed only within it; leading out raises `Errno::EACCES` |
| a link loop | `Errno::ELOOP` after 40 links, as Linux answers |
| a link in an Overlay's lower Layer | read through it; a change that would copy it up raises `Errno::EXDEV` |

A link leading out of an opened directory is refused as a `..` leading out is.

### Ruby Filesystems

A Filesystem written in Ruby answers for what kobako cannot reach inside it.

| Risk | Where it lies |
|---|---|
| its own atomicity | the Filesystem's |
| its time | counts toward `timeout:` uninterrupted |
| a `\` in a name it receives | the Filesystem's to refuse, as Windows reads it as a separator |
| a host link its open meets on Windows | the Filesystem's to refuse, as Windows' Ruby has no `File::NOFOLLOW` |

### Windows Hosts

Windows reads some names differently and holds open directories in place.

| Risk | Where it lies |
|---|---|
| a `\` in a path on a Bind | `Errno::EINVAL`, never asked of the host |
| a name Windows reads as another on a Bind | `Errno::EINVAL`: a stream, a device, a wildcard, or a trailing dot or space |
| a directory a Bind holds open | the host cannot move or remove it until the Bind is collected |
| a Bind's directory opened beneath a Policy, third-party | a path through it raises `Errno::ENOENT`, as a Bind reports no identity there |

### Concurrent Invocations

One Filesystem may serve several Invocations, or several instances may share
state.

| Risk | Where it lies |
|---|---|
| one Overlay's upper shared by another, or mounted on its own | undefined, as overlayfs leaves it; no crash or deadlock |
| one Tmpfs handed to two Filesystem instances in the Rust SDK | undefined under concurrent moves, as each instance has its own locks |
| one Filesystem mounted in Invocations that run at once | a change to one entry waits for the other, until `timeout:` or an interrupt |
| a path whose `..` meets a move landing during its walk | reaches where the directory stands now, or `Errno::EAGAIN` while moves keep landing |

The Wasm Driver makes such a call again a few times before the guest sees
`Errno::EAGAIN`.

## Rust SDK

The Rust SDK mounts an `Arc<Filesystem>` built from a kind in
`kobako::filesystem`. A Filesystem written in Rust implements
`FileSystemType` and the VFS traits of `kobako-runtime`, in place of a Ruby
object.

| Ruby | Rust |
|---|---|
| `Tmpfs.new("a.yml" => yaml)` | `[("a.yml", yaml)].into_iter().collect::<Tmpfs>()` |
| `size:`, `nr_inodes:` | `Tmpfs::with_size`, `Tmpfs::with_nr_inodes` |
| `Bind.new(path)` | `Bind::new(path)` |
| `Overlay.new(lower:, upper:)` | `Overlay::new(lower, Some(upper))` |
| `mount(at, fs, access:)` | `mount(at, fs, Access::Read)` |
| `mount(at, fs) { \|path, access\| ... }` | `mount_with_policy(at, fs, policy)`, read-write |
| `ctx.mount` | `Context::mount` in `eval_with` |

```rust
let tmpfs = [("app.yml", yaml)].into_iter().collect::<Tmpfs>();
let fs = Filesystem::new(Arc::new(tmpfs)).expect("a seeded Tmpfs stands");
sandbox.mount("/config", fs, Access::Read)?;
```

A Policy is a closure, or a type implementing `Policy<MountRequest>`, answering `false` to refuse.
