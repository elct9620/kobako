# Plugin Host

A note-taking host, written in Rust on the [`kobako`](https://crates.io/crates/kobako) SDK crate, runs untrusted mruby **plugins** that batch-edit its notes. Each plugin is arbitrary user code, so it runs inside a `kobako::Sandbox`. It touches the host only through the capabilities the host grants:

```
plugin ─> Notes::Store.open("welcome") ─> Service ─> host Store
       <─ note (Handle) ──────────────────────────── host Note
plugin ─> note.tag / note.each_tag { } ─> Handle ──> host Note (a)
```

a. The same `Note` the Service handed back, never a copy.

This is the Rust counterpart of the Ruby gem's `Kobako::Sandbox`, behind an idiomatic Rust API.

## SDK Conveniences

The example tours three things the SDK assembles for a host. The [`wire-rs`](../wire-rs) example builds the same seam by hand.

| Convenience | The plugin writes | The host gets |
|---|---|---|
| **Service** | `Notes::Store.open("welcome")`, with no import or setup | a call on the object bound as `Notes::Store` |
| **capability Handle** | `note.title`, `note.append`, `note.tag` | every call on the same `Note`, which the plugin can never serialize or forge |
| **block yield** | `note.each_tag { \|name\| … }` | a guest block it drives, one tag at a time |

When the plugin returns the note, the host resolves the returned Handle to the very `Note` it changed, and reads the final state. A `break` in the block ends the iteration, and its value becomes the call's result.

### Execution

`eval` hands back an **`Execution`**, the record of one run. Its parts stay readable whichever way the run ended:

| Part | Holds |
|---|---|
| `Execution::value` | the plugin's value, or what it raised |
| captured output and usage | what the plugin wrote, even before it raised |
| the outer `Err` | only a refusal that never reached the guest |

## Guest Binary

The 0.22 crates release together with kobako 0.32.0, so the host runs the default Guest Binary from that [GitHub Release](https://github.com/elct9620/kobako/releases/tag/v0.32.0).

```bash
cd examples/plugin-rs
curl -LO https://github.com/elct9620/kobako/releases/download/v0.32.0/kobako-0.32.0.wasm
```

## Running

The host takes the Guest Binary first and an optional plugin source second.

```bash
# Default plugin: opens the seeded "welcome" note, edits it, iterates its tags
cargo run -- kobako-0.32.0.wasm

# Your own plugin as the second argument
cargo run -- kobako-0.32.0.wasm 'Notes::Store.open("draft").tag("idea")'

# A Service misuse surfaces in the plugin as a rescuable Kobako::ServiceError
cargo run -- kobako-0.32.0.wasm 'begin; Notes::Store.frobnicate; rescue => e; e.class.to_s; end'

# An uncaught guest exception comes back as a decoded failure, exit code 1
cargo run -- kobako-0.32.0.wasm 'raise ArgumentError, "boom"'
```

## Plugin Reach

The plugin has no I/O, network, or filesystem capability by construction. It reaches only the constant the example binds and the note Handle that constant hands back.

| Reachable as        | Kind       | Methods                                              |
|---------------------|------------|------------------------------------------------------|
| `Notes::Store`      | Service    | `open(id)` — returns a note Handle                   |
| the note Handle     | Handle     | `title`, `body`, `append(text)`, `tag(name)`, `each_tag { \|name\| … }` |

## Options

The caps the host hard-codes are the same knobs the Ruby gem exposes as `Kobako::Sandbox` options.

| Option          | Value        | Purpose                                        |
|-----------------|--------------|------------------------------------------------|
| `timeout`       | 5 s          | Wall-clock cap for one invocation.             |
| `memory_limit`  | 64 MiB       | Guest memory, table, and GC heap growth cap.   |
| `stdout_limit`  | 64 KiB       | Captured-stdout cap.                           |
| `stderr_limit`  | 64 KiB       | Captured-stderr cap.                           |
| `profile`       | `Hermetic`   | Ambient-denial posture: frozen clocks and entropy. |

## Payload Schema

The host objects here implement `ValueReceiver`, so a dispatch arrives as decoded values rather than payload bytes:

| A host that | Implements | Example |
|---|---|---|
| speaks the SDK's bundled MessagePack schema | `ValueReceiver`, the overlay on the byte-level `Receiver` seam | this one |
| speaks its own schema | `Receiver` directly, owning its own bytes | [`fixed-schema-rs`](../fixed-schema-rs) |

## Standalone Workspace

This example is a standalone cargo workspace on the crates.io release, so it builds and runs from this directory alone:

| Needs | Source |
|---|---|
| the crates | crates.io, `kobako = "0.22"` |
| the Guest Binary | the GitHub Release, the only artifact downloaded |
| Rust | 1.98, inherited from the SDK |
