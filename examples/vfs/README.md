# Overlay VFS — a guest `File` that protects the disk

A self-contained script that gives the guest a native-style `File`
constant backed by a host **overlay filesystem**: reads fall through to
the real disk, but every write is intercepted into an in-memory overlay,
so untrusted guest code can freely "edit" files while the disk on the
host stays untouched. It is the [`Kobako::Extension`](../../docs/extensions.md)
companion to the [serverless](../serverless/README.md) and
[async-io](../async-io/README.md) demos — those bind plain Services; this
one installs a full idiom-plus-backend capability.

## The shape, and why it has to be this shape

An Extension teaches the guest a native-style constant by pairing a guest
idiom (`source`) with an optional host `backend`. `Sandbox#install`
composes the two through the existing `#preload` and `#bind` verbs — it
adds no wire, codec, or Guest Binary surface.

The `File` idiom defines only what is pure — `basename` runs in-guest
with no round-trip. `read` and `write` are left undefined, so they fall
through to the bound host backend:

```
File.basename(path)     ->  in-guest, no dispatch
File.read(path)         ->  host: overlay hit? serve it : read the real disk
File.write(path, data)  ->  host: store in the in-memory overlay only
```

The backend is supplied through a **callable** provider
(`-> { OverlayFileSystem.new(root) }`), so `install` resolves a fresh
overlay at the start of every invocation. That single choice buys two
guarantees:

- **Within** one invocation, a write is visible to a later read — the
  guest sees its own changed result.
- **Across** invocations the overlay resets, so a write can never leak
  into the next call, and the real file on disk is never mutated.

A read-through backend also crosses a trust boundary: the guest chooses
the path. `OverlayFileSystem` therefore contains every read within its
root, so `File.read("../../etc/passwd")` cannot escape the example
directory. Bind the least authority the guest needs — never the whole
filesystem.

## Running

The script uses `bundler/inline`, so it resolves its own dependencies on
first run — no `Gemfile` is required in the working directory.

```bash
ruby examples/vfs/app.rb
```

From a clone of the kobako repository, prefix with `bundle exec` so the
local checkout is used instead of the released gem.

## What to observe

The run makes two invocations against the same Sandbox. `sample.txt`
ships next to the script holding `hello from the host disk`.

```
$ ruby examples/vfs/app.rb
vfs overlay demo — a read-through overlay that protects the disk

invocation 1 — write then read (one overlay):
  read  before write : "hello from the host disk\n"   # read-through to disk
  write "patched in memory\n"                       # intercepted into overlay
  read  after  write : "patched in memory\n"  # overlay hit — the changed view
  basename ran in-guest, no round-trip: "sample.txt"

invocation 2 — fresh overlay, read only:
  read               : "hello from the host disk\n"   # overlay reset; disk view again

sample.txt on disk (after both runs):
  "hello from the host disk\n"   # never mutated
```

Three things to read off the trace. Invocation 1's second read returns
the guest's own write, not the disk — the overlay shadows the file.
Invocation 2's read returns the original disk content again, proving the
overlay was rebuilt and the earlier write never leaked. And the final
line reads the real `sample.txt` from the host and finds it unchanged —
the write was contained in memory the whole time.

## Why this is safe

The disk is never a write target: `OverlayFileSystem#write` only mutates
its in-memory Hash. Reads are the only operation that touches the disk,
and they are confined to the overlay root, so a guest-chosen path cannot
read outside the example directory. Per-invocation freshness is not a
convenience here — it is the mechanism that stops one invocation's writes
from being visible to the next.
