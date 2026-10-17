# kobako-runtime

Engine-neutral host runtime contract for
[kobako](https://github.com/elct9620/kobako), an in-process Wasm
sandbox for running untrusted mruby scripts.

A kobako host drives a Guest Binary through a wasm engine. This crate
is where the two meet, free of any engine or frontend type, so the
engine stays swappable.

| Module | What it declares |
|---|---|
| `runtime` | the `Runtime` trait: one guest invocation on a fresh instance in, its observable `Snapshot` out |
| `snapshot` | each invocation's `Completion` (outcome or trap), both output `Capture`s, and `Usage` |
| `error` | the neutral failure types: `Trap`, `SetupError`, and `InvokeError` over both |
| `profile` | the isolation `Profile` a runtime declares |
| `grant` | the `Grant` a frontend hands each invocation, and the `GrantKind`s a runtime supports |
| `mount` | the `Mount` placing a `Filesystem` at a guest path, with its `Access` and `Policy` |
| `filesystem` | the `Filesystem` behind a Mount, and the `Memory`, `Temporary`, `Directory`, and `Overlay` kobako ships |
| `dispatch` / `yielder` | the `DispatchHandler` and `Yielder` traits a frontend supplies for guest→host dispatch and block-yield re-entry |

Engine implementations such as `kobako-wasmtime` implement `Runtime`.
Host frontends such as the kobako Ruby gem's native ext map the neutral
types onto their own language surface.

## Usage

Add the crate to an engine or a frontend that speaks the contract.

```toml
[dependencies]
kobako-runtime = "0.18.0" # x-release-please-version
```

## License

Licensed under [Apache-2.0](https://www.apache.org/licenses/LICENSE-2.0).
