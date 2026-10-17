# Overlay VFS

A self-contained script that gives the guest a native-style `Disk`
constant backed by a host **overlay filesystem**. Untrusted guest code
can freely "edit" files while the disk on the host stays untouched. It
is the [`Kobako::Extension`](../../docs/guides/extensions.md) companion
to the [serverless](../serverless/README.md) and
[async-io](../async-io/README.md) demos, which bind plain Services.

```
guest Disk.read ─> host OverlayFileSystem ─┬─ written before ─> in-memory overlay
                                           └─ otherwise      ─> real disk, read only
```

## Extension Shape

An Extension teaches the guest a native-style constant by pairing a guest
idiom (`source`) with an optional host `backend`. `Sandbox#install`
composes the two through the existing `#preload` and `#bind` verbs — it
adds no wire, codec, or Guest Binary surface.

```
Kobako::Extension ─┬─ source  (guest idiom)  ─> #preload
                   └─ backend (host object)  ─> #bind
```

### Idiom and backend

The `Disk` idiom defines only what is pure — `basename` runs in-guest
with no round-trip. `read` and `write` are left undefined, so they fall
through to the bound host backend:

```
Disk.basename(path)     ->  in-guest, no dispatch
Disk.read(path)         ->  host: overlay hit? serve it : read the real disk
Disk.write(path, data)  ->  host: store in the in-memory overlay only
```

The idiom is named `Disk` so it never meets the Guest Binary's own
`File`, which a Host App reaches through `Sandbox#mount` instead.

### Fresh overlays

The backend is supplied through a **callable** provider, so `install`
resolves a fresh overlay at the start of every invocation. That single
choice buys two guarantees:

```ruby
Kobako::Extension::Backend.new(path: "Disk", provider: -> { OverlayFileSystem.new(root) })
```


- **Within** one invocation, a write is visible to a later read — the
  guest sees its own changed result.
- **Across** invocations the overlay resets, so a write can never leak
  into the next call, and the real file on disk is never mutated.

### Path containment

A read-through backend also crosses a trust boundary: the guest chooses
the path. `OverlayFileSystem` therefore contains every read within its
root. Bind the least authority the guest needs — never the whole
filesystem.

| Guest asks for | Backend answers |
|---|---|
| `Disk.read("sample.txt")` | the file beneath the example directory |
| `Disk.read("../../etc/passwd")` | refused, as a `Kobako::ServiceError` |

## Running

The script uses `bundler/inline`, so it resolves its own dependencies on
first run — no `Gemfile` is required in the working directory.

```bash
ruby examples/vfs/app.rb
```

From a clone of the kobako repository, prefix with `bundle exec` so the
local checkout is used instead of the released gem.

## Expected Output

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

escape attempt — read outside the overlay root:
  Disk.read("../../etc/passwd") : rejected   # contained by the host backend

sample.txt on disk (after both runs):
  "hello from the host disk\n"   # never mutated
```

Three things to read off the trace. Invocation 1's second read returns
the guest's own write, not the disk — the overlay shadows the file.
Invocation 2's read returns the original disk content again, proving the
overlay was rebuilt and the earlier write never leaked. And the final
line reads the real `sample.txt` from the host and finds it unchanged —
the write was contained in memory the whole time.

## Safety Properties

Each guest operation reaches the host only as far as the table says.
Per-invocation freshness is not a convenience here — it is the mechanism
that stops one invocation's writes from being visible to the next.

| Operation | Reaches |
|---|---|
| `Disk.write` | the in-memory overlay only, never the disk |
| `Disk.read` | the overlay, or the disk within the overlay root |
| a path outside the root | nothing; the backend refuses it |
