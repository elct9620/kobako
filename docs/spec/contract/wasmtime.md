# wasmtime engine settings

What a Frontend hands the bundled wasmtime engine: the caps and isolation it
asks for. The engine itself is reached through the engine seams, so the
settings are all this crate offers by name.

## Includes

- `crates/kobako-wasmtime/src/**/*.rs`

## `Config`

The caps and the isolation profile a Frontend asks the engine for.

```rust
pub struct Config {}
```
