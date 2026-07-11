# kobako-baker

Build-time pre-initializer for
[kobako](https://github.com/elct9620/kobako) Guest Binaries — bakes
the canonical boot state into a linked guest artifact via
[wasmtime-wizer](https://crates.io/crates/wasmtime-wizer).

`bake` executes the module's `wizer.initialize` export (the
`MrbGuest::bake_boot` body) against a deterministic linker — the WASI
surface wasi-libc's reactor `_initialize` touches answers constants
(mirroring kobako's ambient denial), `env::__kobako_dispatch`
traps, and any other import called during boot aborts the bake — then
snapshots the booted interpreter into the artifact's data segments.
Identical inputs produce identical baked bytes, so a double-bake
byte-identity check gates reproducibility.

A host that implements kobako ABI v2 instantiates the baked module
afresh per invocation; instantiation rides wasmtime's copy-on-write
image mapping, so every invocation receives the booted mruby VM
without paying boot.

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
kobako-baker = "0.9.0" # x-release-please-version
```

```rust
let baked = kobako_baker::bake(&linked_wasm_bytes)?;
```

## License

Apache-2.0
