# kobako-baker

Build-time pre-initializer for
[kobako](https://github.com/elct9620/kobako) Guest Binaries — bakes
the canonical boot state into a linked guest artifact via
[wasmtime-wizer](https://crates.io/crates/wasmtime-wizer).

`bake` executes the module's `wizer.initialize` export (the
`MrbGuest::bake_boot` body) against a deterministic linker, then
snapshots the booted interpreter into the artifact's data segments.

| Import reached during boot | What the linker answers |
|---|---|
| the WASI surface wasi-libc's reactor `_initialize` touches | constants, mirroring kobako's ambient denial |
| `env::__kobako_dispatch` | a trap |
| anything else | the bake aborts |

Identical inputs produce identical baked bytes, so a double-bake
byte-identity check gates reproducibility.

## What the host gets

A kobako host instantiates the baked module afresh per invocation.
Instantiation rides wasmtime's copy-on-write image mapping, so every
invocation receives the booted mruby VM without paying boot.

```text
kobako.wasm (baked)  ──copy-on-write──>  invocation 1, 2, 3 …
                                          each a booted VM, no boot cost
```

## Usage

As the CLI (what kobako's own Stage C runs):

```console
$ kobako-baker input.wasm output.wasm
```

As a library, for third-party guest shells built on
[kobako-mruby](https://crates.io/crates/kobako-mruby) /
[kobako-core](https://crates.io/crates/kobako-core):

```toml
[dependencies]
kobako-baker = "0.16.0" # x-release-please-version
```

```rust
let baked = kobako_baker::bake(&linked_wasm_bytes)?;
```

## License

Apache-2.0
