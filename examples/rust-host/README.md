# Rust Host

A kobako host assembled in Rust directly on the published crates, without the Ruby gem. The three crates are exactly the standard surface a non-Ruby embedder consumes: `kobako-wasmtime` provides the `Driver` that runs a prebuilt Guest Binary, `kobako-runtime` is the engine-neutral contract the driver implements (`Runtime`, `Snapshot`, `Profile`, the dispatch traits), and `kobako-codec` encodes and decodes the SPEC wire the host side owns — the stdin frames going in and the `Outcome` bytes coming back.

The `minimal` bin is the smallest complete host: one `#eval`-equivalent invocation with nothing registered, reading back the decoded return value (or guest `Panic`), both capture channels, and the resource usage, and attributing every failure channel (`SetupError`, `Trap`) the way a frontend maps them onto its own error surface. The Ruby gem's `Kobako::Sandbox` is this same assembly plus the Service registry, Handle table, and snippet conveniences.

The `services` bin adds the guest→host half: Frame 1 registers the `MyService::KV` constant path, and a `DispatchHandler` answers each call the guest makes against it — decoding the `Request` envelope, routing to an in-process store, and encoding a `Response`. It honours the dispatch contract the Ruby gem's `Transport::Dispatcher` pins: the handler never fails, folding every failure into a `Response::Err` fault that surfaces in the guest as a rescuable exception rather than a wasm trap. What the Ruby gem layers on top of this seam — the Handle table for non-wire-representable values, block yields, nested dispatch — is exactly the glue a fuller embedder SDK would provide.

This example is a standalone cargo workspace depending on the crates.io releases, so it builds and runs from this directory alone — the Guest Binary is the only artifact it needs.

## Getting a Guest Binary

Either download the platform-agnostic artifact attached to a [GitHub Release](https://github.com/elct9620/kobako/releases) (`kobako-<version>.wasm`), or build it from a clone of this repository:

```bash
bundle exec rake wasm:build   # produces data/kobako.wasm
```

## Running

```bash
cd examples/rust-host

# Default demo source: stdout capture + a structured return value
cargo run --bin minimal -- ../../data/kobako.wasm

# Your own mruby source as the second argument
cargo run --bin minimal -- ../../data/kobako.wasm '[1, 2, 3].sum'

# Guest failures come back as decoded Panic records, engine faults as traps
cargo run --bin minimal -- ../../data/kobako.wasm 'raise ArgumentError, "boom"'
cargo run --bin minimal -- ../../data/kobako.wasm 'loop { }'   # trips the 5s wall-clock cap

# Guest→host dispatch against the in-process MyService::KV store
cargo run --bin services -- ../../data/kobako.wasm
cargo run --bin services -- ../../data/kobako.wasm 'MyService::KV.set("n", 41); MyService::KV.get("n") + 1'
```

## Arguments

| Argument | Purpose                                                        | Default          |
|----------|----------------------------------------------------------------|------------------|
| 1st      | Path to the Guest Binary (`kobako.wasm`).                      | required         |
| 2nd      | mruby source to evaluate.                                      | built-in demo    |

The caps the bin hard-codes (5 s wall clock, 64 MiB linear memory, 64 KiB per capture channel, `Hermetic` profile) are the same knobs the Ruby gem exposes as `Kobako::Sandbox` options.
