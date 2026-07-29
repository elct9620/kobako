# Plugin Host (Rust SDK)

A note-taking host, written in Rust on the [`kobako`](https://crates.io/crates/kobako) SDK crate, that runs untrusted mruby **plugins** to batch-edit its notes. The plugin is arbitrary user code, so it runs inside a `kobako::Sandbox` and can touch the host only through the capabilities the host chose to grant — the Rust counterpart of what the Ruby gem's `Kobako::Sandbox` does, behind an idiomatic Rust API.

This is the narrative tour of the three SDK conveniences a lower-level host would otherwise assemble by hand (see the [`wire-rs`](../wire-rs) example for that seam):

A **Service** is a host object the plugin calls like a constant. The host binds one as `Notes::Store`, and the plugin reaches it as `Notes::Store.open("welcome")` with no import or setup.

A **capability Handle** is a live host object the plugin holds but can never serialize or forge. `Store.open` hands back a `Note`; the plugin calls `note.title`, `note.append`, and `note.tag` on it, and every call dispatches back to the same host object. When the plugin returns the note, the host resolves the returned Handle back to the very `Note` it mutated and reads the final state — the Rust spelling of restore-to-original-object.

A **block yield** runs a guest block the host drives. `note.each_tag { |name| … }` yields each tag into the plugin's block one at a time; a `break` in the block ends the iteration, and its value becomes the call's result.

What `eval` hands back is an **`Execution`** — the record of one run. Whether the plugin returned a value or raised rides `Execution::value`, while the run's captured output and usage stay readable on either arm, so the host prints what a failing plugin managed to write before it raised. Only a refusal that never reached the guest is the outer `Err`.

## Getting a Guest Binary

Either download the platform-agnostic artifact attached to a [GitHub Release](https://github.com/elct9620/kobako/releases) (`kobako-<version>.wasm`), or build it from a clone of this repository:

```bash
bundle exec rake wasm:build   # produces data/kobako.wasm
```

## Running

```bash
cd examples/plugin-rs

# Default plugin: opens the seeded "welcome" note, edits it, iterates its tags
cargo run -- ../../data/kobako.wasm

# Your own plugin as the second argument
cargo run -- ../../data/kobako.wasm 'Notes::Store.open("draft").tag("idea")'

# A Service misuse surfaces in the plugin as a rescuable Kobako::ServiceError
cargo run -- ../../data/kobako.wasm 'begin; Notes::Store.frobnicate; rescue => e; e.class.to_s; end'

# An uncaught guest exception comes back as a decoded failure, exit code 1
cargo run -- ../../data/kobako.wasm 'raise ArgumentError, "boom"'
```

## What the plugin can reach

The plugin has no I/O, network, or filesystem capability by construction — only the host constant the example binds and the note Handle it hands back.

| Reachable as        | Kind       | Methods                                              |
|---------------------|------------|------------------------------------------------------|
| `Notes::Store`      | Service    | `open(id)` — returns a note Handle                   |
| the note Handle     | Handle     | `title`, `body`, `append(text)`, `tag(name)`, `each_tag { \|name\| … }` |

## Options

The caps the host hard-codes are the same knobs the Ruby gem exposes as `Kobako::Sandbox` options.

| Option          | Value        | Purpose                                        |
|-----------------|--------------|------------------------------------------------|
| `timeout`       | 5 s          | Wall-clock cap for one invocation.             |
| `memory_limit`  | 64 MiB       | Guest linear-memory cap.                       |
| `stdout_limit`  | 64 KiB       | Captured-stdout cap.                           |
| `stderr_limit`  | 64 KiB       | Captured-stderr cap.                           |
| `profile`       | `Hermetic`   | Ambient-denial posture: frozen clocks and entropy. |

The host objects here implement `ValueReceiver`, the overlay the SDK's bundled MessagePack schema puts on its byte-level `Receiver` seam — so a dispatch arrives as decoded values rather than payload bytes. A host speaking its own schema implements `Receiver` directly and owns its own bytes instead; [`fixed-schema-rs`](../fixed-schema-rs) is that case.

This example is a standalone cargo workspace depending on the crates.io release, so it builds and runs from this directory alone — the Guest Binary is the only artifact it needs. It requires Rust 1.86, inherited from the SDK.
