# Wire Host

A kobako host assembled by hand from the published wire crates, **without** the [`kobako`](https://crates.io/crates/kobako) SDK. Where the [`plugin-rs`](../plugin-rs) example reaches for the SDK's `Sandbox`, this one drives the wire underneath it directly. Follow it when you port a kobako frontend to another host language: the crates happen to be Rust, but the lesson is the wire.

```
Preamble ─> Driver ─> guest ─ Call ─> DispatchHandler ─> Reply
                        └─────────── Outcome ─> host decodes the ok body
```

## Crate Split

Four crates are the whole toolkit, and the split between the last two is the lesson.

| Crate | Owns |
|---|---|
| `kobako-wasmtime` | the `Driver`, running a prebuilt Guest Binary on a fresh instance per invocation |
| `kobako-runtime` | the engine-neutral contract: `Runtime`, `Snapshot`, the dispatch traits |
| `kobako-transport` | the **core envelope**: the `Preamble`, the `Call` and its `Reply`, the `Outcome` |
| `kobako-codec` | only the payload riding *inside* an envelope, under this host's schema |

A port to another host language should copy that boundary first. Everything above the codec stays the same whatever schema you carry, and only the payload half is yours to replace.

## Dispatch

The host drives one `#eval`-equivalent invocation. Its `Preamble` binds a `MyService::KV` constant path, the rest left at its default. A `DispatchHandler` reads the routing fields the runtime already decoded, routes each Call to an in-process store, and answers with a `Reply`.

| Rule | Why |
|---|---|
| the handler never fails | a failure rides the Reply's fault arm, rescuable in the guest, never a trap |
| a Fault is typed on the envelope | refusing a call reaches no codec at all |
| no malformed-request arm | a Call whose envelope the runtime could not read never arrives |

The Ruby gem's `Transport::Dispatcher` pins the same first rule. The SDK layers its glue on top of this seam, and `plugin-rs` shows it from the other side. That glue is the Handle table for non-wire values, block yields, snippet replay, and seal-once registration. Reach for the SDK unless you need this level of control.

## Guest Binary

The 0.19 crates release together with kobako 0.29.0, so the host runs the default Guest Binary from that [GitHub Release](https://github.com/elct9620/kobako/releases/tag/v0.29.0).

```bash
cd examples/wire-rs
curl -LO https://github.com/elct9620/kobako/releases/download/v0.29.0/kobako-0.29.0.wasm
```

## Running

The host takes the Guest Binary first and an optional mruby source second.

```bash
# Default demo: a store round-trip, a rescued Service fault, and a miss
cargo run -- kobako-0.29.0.wasm

# Your own mruby source as the second argument
cargo run -- kobako-0.29.0.wasm 'MyService::KV.set("n", 41); MyService::KV.get("n") + 1'

# A guest failure comes back as a decoded Panic; an engine fault as a trap
cargo run -- kobako-0.29.0.wasm 'raise ArgumentError, "boom"'
cargo run -- kobako-0.29.0.wasm 'loop { }'   # trips the 5s wall-clock cap
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
