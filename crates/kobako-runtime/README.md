# kobako-runtime

Engine-neutral host runtime contract for
[kobako](https://github.com/elct9620/kobako), an in-process Wasm
sandbox for running untrusted mruby scripts.

A kobako host drives a Guest Binary through a wasm engine; this crate
is the surface where the two meet, free of any engine or frontend
type, so the engine stays swappable:

- `runtime` — the `Runtime` trait: one guest invocation on a fresh
  instance in, its observable `Snapshot` out
- `snapshot` — the per-invocation observables: `Completion` (outcome
  or trap), the two output `Capture`s, and resource `Usage`, uniform
  across success and trap
- `error` — the neutral failure channels: `Trap` (engine fault) and
  `SetupError` (the invocation never started)
- `dispatch` / `yielder` — the `DispatchHandler` and `Yielder` traits
  a frontend supplies for guest→host dispatch and block-yield re-entry

Engine implementations (such as `kobako-wasmtime`) implement
`Runtime`; host frontends (such as the kobako Ruby gem's native ext)
map the neutral types onto their own language surface.

## Usage

```toml
[dependencies]
kobako-runtime = "0.10.2" # x-release-please-version
```

## License

Apache-2.0
