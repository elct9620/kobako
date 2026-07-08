# Wire Host (low-level, no SDK)

A kobako host assembled by hand from the three published wire crates, **without** the [`kobako`](https://crates.io/crates/kobako) SDK. Where the [`plugin-rs`](../plugin-rs) example reaches for the SDK's `Sandbox`, this one exposes the seam underneath it: the SPEC wire the host side owns, driven directly. It is the reference to follow when you are porting a kobako frontend to another host language and need to see the frames and envelopes driven concretely — the crates happen to be Rust, but the lesson is the wire.

Three crates are the whole toolkit. `kobako-wasmtime` provides the `Driver` that runs a prebuilt Guest Binary on a fresh instance per invocation. `kobako-runtime` is the engine-neutral contract the driver implements (`Runtime`, `Snapshot`, the dispatch traits). `kobako-codec` encodes and decodes the SPEC wire the host side speaks — the stdin frames going in and the `Outcome` bytes coming back.

The host drives one `#eval`-equivalent invocation. Frame 1 registers a `MyService::KV` constant path by hand — an empty registration is simply an empty msgpack array, never an absent frame — and a `DispatchHandler` answers each call the guest makes against it: decode the `Request` envelope, route it to an in-process store, encode a `Response`. It honours the one hard rule of the dispatch contract that the Ruby gem's `Transport::Dispatcher` also pins: the handler never fails, folding every failure into a `Response::Err` fault that surfaces in the guest as a rescuable exception rather than a wasm trap.

What the SDK layers on top of this seam — the Handle table for non-wire values, block yields, snippet replay, seal-once registration — is exactly the glue `plugin-rs` shows from the other side. Reach for the SDK unless you need this level of control.

## Getting a Guest Binary

Either download the platform-agnostic artifact attached to a [GitHub Release](https://github.com/elct9620/kobako/releases) (`kobako-<version>.wasm`), or build it from a clone of this repository:

```bash
bundle exec rake wasm:build   # produces data/kobako.wasm
```

## Running

```bash
cd examples/wire-rs

# Default demo: a store round-trip, a rescued Service fault, and a miss
cargo run -- ../../data/kobako.wasm

# Your own mruby source as the second argument
cargo run -- ../../data/kobako.wasm 'MyService::KV.set("n", 41); MyService::KV.get("n") + 1'

# A guest failure comes back as a decoded Panic; an engine fault as a trap
cargo run -- ../../data/kobako.wasm 'raise ArgumentError, "boom"'
cargo run -- ../../data/kobako.wasm 'loop { }'   # trips the 5s wall-clock cap
```

## Options

The caps the host hard-codes are the same knobs the Ruby gem exposes as `Kobako::Sandbox` options.

| Option              | Value      | Purpose                                            |
|---------------------|------------|----------------------------------------------------|
| `timeout`           | 5 s        | Wall-clock cap for one invocation.                 |
| memory limit        | 64 MiB     | Guest linear-memory cap.                           |
| `stdout_limit_bytes`| 64 KiB     | Captured-stdout cap.                               |
| `stderr_limit_bytes`| 64 KiB     | Captured-stderr cap.                               |
| `profile`           | `Hermetic` | Ambient-denial posture: frozen clocks and entropy. |

This example is a standalone cargo workspace depending on the crates.io releases, so it builds and runs from this directory alone — the Guest Binary is the only artifact it needs.
