# Overlay VFS

A self-contained script that mounts a host directory for the guest's own
`File`, under a writable layer that forgets every change. Untrusted guest
code can freely "edit" files while the directory on the host stays
untouched.

```
guest File.write("/disk/sample.txt") ─> Overlay ─┬─ Temporary  :read_write ─> takes the copy
guest File.read("/disk/sample.txt")  ─>          └─ Directory  :read       ─> examples/vfs
```

## Mount Shape

`Sandbox#mount` places a Filesystem at a guest path. An Overlay stacks
Layers into one view, the first added on top. A change to a file in the
read-only Directory first copies it up into the Temporary Layer.

```ruby
overlay = Kobako::Filesystem::Overlay.new
  .layer(Kobako::Filesystem::Temporary.new, access: :read_write) # empty every run
  .layer(Kobako::Filesystem::Directory.new(__dir__))              # never written
sandbox.mount("/disk", overlay, access: :read_write) do |path, access|
  access == :read || path == "/disk/sample.txt"
end
```

### Fresh Layer

A Temporary starts empty at every invocation. That one choice gives two
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

invocation 1 — write then read (one Temporary Layer):
  read  before write : "hello from the host disk\n"   # the host directory
  write "patched in memory\n"                       # copied up into the Temporary Layer
  read  after  write : "patched in memory\n"  # the changed view
  File.basename ran in the guest: "sample.txt"

invocation 2 — a fresh Temporary Layer, read only:
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
fresh Temporary Layer is the mechanism that keeps one invocation's writes
from the next.

| Operation | Reaches |
|---|---|
| `File.write` | the Temporary Layer only, never the host |
| `File.read` | the Temporary Layer, or the directory beneath the mount |
| a path the block refuses or outside the mount | nothing |

The [README](../../README.md) lists every Filesystem kind, and the
[security guide](../../docs/guides/security.md) covers what a mount block
judges.
