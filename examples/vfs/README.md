# Overlay VFS

A self-contained script that mounts a host directory for the guest's own
`File`, under a writable layer that forgets every change. Untrusted guest
code can freely "edit" files while the directory on the host stays
untouched.

```
File.write ─> Overlay ─┬─ upper: Tmpfs (a)
File.read  ─>          └─ lower: Bind  (b)
```

a. Takes the copy of a changed file, for this run only.
b. The `examples/vfs` directory, never written.

## Mount Shape

`Sandbox#mount` places a Filesystem at a guest path. An Overlay stacks a
Tmpfs over read-only lower Layers, as Linux's overlayfs does. A change to a
file in the Bind first copies it up into the Tmpfs.

Every invocation reaches the same Filesystem a Sandbox mounts. So the
Sandbox mounts the host directory read-only, and each run replaces it
through `ctx.mount` with an Overlay over a Tmpfs of its own.

```ruby
allow = ->(path, access) { access == :read || path == "/disk/sample.txt" }
disk = Kobako::Filesystem::Bind.new(__dir__)
sandbox.mount("/disk", disk, &allow)

sandbox.eval(source) do |ctx|
  overlay = Kobako::Filesystem::Overlay.new(
    lower: [disk],                       # never written
    upper: Kobako::Filesystem::Tmpfs.new # takes every change, this run only
  )
  ctx.mount("/disk", overlay, access: :read_write, &allow)
end
```

### Fresh Tmpfs

Each run starts on a new, empty Tmpfs. That one choice gives two
guarantees:

| Scope | A write |
|---|---|
| within one invocation | is visible to a later read; the guest sees its own result |
| across invocations | is gone, and the host file was never changed |

### Mount Block

The guest chooses the path, so the block decides each open and each
change in the access's place. A falsy answer refuses it.

| Guest asks for | The guest sees |
|---|---|
| `File.read("/disk/sample.txt")` | the file beneath the example directory |
| `File.write("/disk/app.rb", "x")` | `Errno::EACCES`, refused by the block |
| `File.read("/disk/../../etc/passwd")` | `Errno::EACCES`, as it climbs out of the mount |
| `File.read("/etc/passwd")` | `Errno::ENOENT`, as nothing is mounted there |

## Running

The script uses `bundler/inline`, so it resolves its own dependencies on
first run; no `Gemfile` is required in the working directory.

```bash
ruby examples/vfs/app.rb
```

From a clone of the kobako repository, prefix with `bundle exec` so the
local checkout is used instead of the released gem.

## Expected Output

The run makes three invocations against the same Sandbox. `sample.txt`
ships next to the script holding `hello from the host disk`.

```
$ ruby examples/vfs/app.rb
vfs overlay demo — the guest's File over a host directory it cannot change

invocation 1 — write then read (its own Tmpfs):
  read  before write : "hello from the host disk\n"   # the host directory
  write "patched in memory\n"                       # copied up into the Tmpfs
  read  after  write : "patched in memory\n"  # the changed view
  File.basename ran in the guest: "sample.txt"

invocation 2 — a new Tmpfs, read only:
  read               : "hello from the host disk\n"   # the host directory again

refusals — what the mount does not grant:
  write app.rb    : Errno::EACCES
  climb out       : Errno::EACCES
  outside a mount : Errno::ENOENT

sample.txt on the host (after every run):
  "hello from the host disk\n"   # never changed
```

Invocation 1's second read returns the guest's own write, not the host
file. Invocation 2's read returns the original again, so the earlier write
never leaked. The last line reads the real `sample.txt` and finds it
unchanged.

## Safety Properties

Each guest operation reaches the host only as far as the table says. The
Tmpfs each run mounts is what keeps one invocation's writes from the next.

| Operation | Reaches |
|---|---|
| `File.write` | the run's Tmpfs only, never the host |
| `File.read` | the run's Tmpfs, or the directory beneath the mount |
| a path the block refuses or outside the mount | nothing |

The mount block judges each open and each change by guest path, and it can
only refuse what the mount's access already allows.
