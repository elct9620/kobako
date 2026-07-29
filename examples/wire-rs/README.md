# Wire Host (low-level, no SDK)

A kobako host assembled by hand from the published wire crates, **without** the [`kobako`](https://crates.io/crates/kobako) SDK. Where the [`plugin-rs`](../plugin-rs) example reaches for the SDK's `Sandbox`, this one exposes the seam underneath it: the SPEC wire the host side owns, driven directly. It is the reference to follow when you are porting a kobako frontend to another host language and need to see the frames and envelopes driven concretely — the crates happen to be Rust, but the lesson is the wire.

Four crates are the whole toolkit, and the split between the last two is the lesson. `kobako-wasmtime` provides the `Driver` that runs a prebuilt Guest Binary on a fresh instance per invocation. `kobako-runtime` is the engine-neutral contract the driver implements (`Runtime`, `Snapshot`, the dispatch traits). `kobako-transport` owns the **core envelope** every kobako assembly shares — the frames, the `Call` and its `Reply`, the `Outcome`. `kobako-codec` owns only what rides *inside* an envelope, the payload, under the schema this host happens to speak.

That boundary is what a port to another host language should copy first: everything above stays the same whatever schema you carry, and only the payload half is yours to replace.

The host drives one `#eval`-equivalent invocation. Frame 1 registers a `MyService::KV` constant path — an empty registration is an empty list, never an absent frame — and a `DispatchHandler` answers each Call the guest makes against it: read the routing fields the runtime already decoded, route to an in-process store, answer with a `Reply`. It honours the one hard rule of the dispatch contract that the Ruby gem's `Transport::Dispatcher` also pins: the handler never fails, folding every failure into the Reply's fault arm, which surfaces in the guest as a rescuable exception rather than a wasm trap.

A Fault is typed on the envelope rather than encoded in the payload, so refusing a call reaches no codec at all — and a Call whose envelope the runtime could not read never arrives here, which is why the handler has no malformed-request arm of its own.

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
| `timeout`       | 5 s        | Wall-clock cap for one invocation.                 |
| `memory_limit`  | 64 MiB     | Guest linear-memory cap.                           |
| `stdout_limit`  | 64 KiB     | Captured-stdout cap.                               |
| `stderr_limit`  | 64 KiB     | Captured-stderr cap.                               |
| `profile`       | `Hermetic` | Ambient-denial posture: frozen clocks and entropy. |

This example is a standalone cargo workspace depending on the crates.io releases, so it builds and runs from this directory alone — the Guest Binary is the only artifact it needs.
