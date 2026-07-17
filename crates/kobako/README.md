# kobako

Rust host SDK for [kobako](https://github.com/elct9620/kobako), an
in-process Wasm sandbox for running untrusted mruby scripts.

`Sandbox` composes the published tiers (`kobako-codec` wire,
`kobako-runtime` contract, `kobako-wasmtime` driver) into the same
host behavior contract the kobako Ruby gem exposes, kept aligned by a
differential parity harness rather than by mirrored API shapes:

- `Sandbox` — one guest per instance: `define` / `bind` / `preload`
  fill the registration tables until the first invocation seals them,
  `eval` / `run` execute on a fresh guest instance and return a
  decoded wire `Value` or a taxonomy `Error`
- `Receiver` — the host object a guest dispatch resolves its target
  to, reached as `MyService::KV` or through a capability
  Handle, with a `respond_to_guest` narrowing predicate and `Fault`
  as its refusal channel
- `Yielder` — the host-side stand-in for a guest-supplied block,
  riding the `block` parameter; each call is a synchronous yield
  round-trip into the in-flight guest
- `Handles` — the per-invocation capability-Handle table: stateful
  host objects cross as opaque tokens the guest can call back into

The `Sandbox` runs a prebuilt Guest Binary (`kobako.wasm`) at runtime;
no mruby toolchain is needed to build an embedder.

## Usage

```toml
[dependencies]
kobako = "0.10.1" # x-release-please-version
```

```rust
use kobako::{Options, Sandbox};

fn main() -> Result<(), kobako::Error> {
    // Load a prebuilt Guest Binary. Options::default() is secure by
    // default: no caps, hermetic isolation (frozen clocks and entropy).
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

Apache-2.0
