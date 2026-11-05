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
another's. The bundled Guest Binaries import no clock, so no script can time
that. A third-party guest on `permissive`, or a script a Service hands a clock,
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
| list a directory | `fd_readdir` |
| make a directory | `path_create_directory` |
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
| a directory the guest lists in steps, third-party | a Tmpfs lists as it stands; others hold entries |
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
