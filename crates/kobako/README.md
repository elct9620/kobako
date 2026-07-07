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
- `Member` — the host object a guest reaches as
  `<Namespace>::<Member>`, with a `respond_to_guest` narrowing
  predicate and `Fault` as its refusal channel
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
kobako = "0.7.0" # x-release-please-version
```

## License

Apache-2.0
