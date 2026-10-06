# kobako

Rust host SDK for [kobako](https://github.com/elct9620/kobako), an
in-process Wasm sandbox for running untrusted mruby scripts.

`Sandbox` composes the published tiers into the same host behavior
contract the kobako Ruby gem exposes. A differential parity harness keeps
the two aligned, rather than mirrored API shapes.

```
kobako::Sandbox
  ├── kobako-transport   envelope
  ├── kobako-codec       payload
  ├── kobako-runtime     contract
  └── kobako-wasmtime    driver
```

The `Sandbox` runs a prebuilt Guest Binary (`kobako.wasm`) at runtime, so
no mruby toolchain is needed to build an embedder.

## Core Types

Four types carry the host side of an invocation.

| Type | What it is |
|---|---|
| `Sandbox` | one guest per instance |
| `Receiver` | the host object a guest dispatch resolves to |
| `Yielder` | the host-side stand-in for a guest-supplied block |
| `Handles` | the per-invocation capability-Handle table |

`define`, `bind` and `preload` fill the registration tables until the
first invocation seals them. `eval` and `run` execute on a fresh guest
instance and return a decoded wire `Value` or a taxonomy `Error`.

A guest reaches a `Receiver` as `MyService::KV` or through a capability
Handle. Its `respond_to_guest` predicate narrows what the guest may call,
and `Fault` is how it refuses.

A `Yielder` rides the `block` parameter, and each call is a synchronous
yield round-trip into the in-flight guest. Through `Handles`, stateful
host objects cross as opaque tokens the guest can call back into.

## Usage

```toml
[dependencies]
kobako = "0.17.0" # x-release-please-version
```

```rust
use kobako::{Options, Sandbox};

fn main() -> Result<(), kobako::Error> {
    // Load a prebuilt Guest Binary. Options::default() is secure by
    // default: a 60 s deadline, 1 MiB for memory and each output
    // channel, hermetic isolation (frozen clocks and entropy).
    let mut sandbox = Sandbox::new("kobako.wasm", Options::default())?;

    // Run untrusted mruby on a fresh instance; the last expression
    // comes back as a decoded wire Value.
    let squares = sandbox.eval("[1, 2, 3].map { |n| n * n }")?;
    println!("{squares:?}");
    Ok(())
}
```

Bind host Services with `Sandbox::bind` and pass capability Handles
through the `Receiver` seam to let guest code call back into the host.

## License

Licensed under [Apache-2.0](https://github.com/elct9620/kobako/blob/main/LICENSE).
