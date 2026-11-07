# kobako-baker

Build-time pre-initializer for
[kobako](https://github.com/elct9620/kobako) Guest Binaries. It bakes the
canonical boot state into a linked guest artifact via
[wasmtime-wizer](https://crates.io/crates/wasmtime-wizer).

```text
linked guest --bake--> baked guest (booted mruby VM in its data segments)
```

## Boot Linker

`bake` runs the module's `wizer.initialize` export, the `MrbGuest::bake_boot`
body, against a deterministic linker. It then snapshots the booted
interpreter into the artifact's data segments. The linker answers each import
boot reaches as follows.

| Import reached during boot | What the linker answers |
|---|---|
| the WASI surface wasi-libc's `_initialize` touches | constants, as kobako's ambient denial |
| `env::__kobako_upcall` | a trap |
| anything else | the bake aborts |

Identical inputs produce identical baked bytes, so a double-bake byte-identity
check gates reproducibility.

## Baked Instances

A kobako host instantiates the baked module afresh per invocation.
Instantiation rides wasmtime's copy-on-write image mapping, so every
invocation receives the booted mruby VM without paying boot.

```text
kobako.wasm (baked) --copy-on-write--> invocation 1, 2, 3 ...
                                       each a booted VM, no boot cost
```

## Usage

kobako's own build runs the CLI on its linked Guest Binary.

```console
$ kobako-baker input.wasm output.wasm
```

A third-party guest shell built on
[kobako-mruby](https://crates.io/crates/kobako-mruby) and
[kobako-core](https://crates.io/crates/kobako-core) calls it as a library.

```toml
[dependencies]
kobako-baker = "0.20.0" # x-release-please-version
```

```rust
let baked = kobako_baker::bake(&linked_wasm_bytes)?;
```

## License

Licensed under [Apache-2.0](https://www.apache.org/licenses/LICENSE-2.0).
