# wasmtime engine

The bundled engine a Frontend builds from a Guest Binary and the caps and
isolation it asks for. It answers through the engine seams, so a Frontend needs
by name only how to build one.

## Includes

- `crates/kobako-wasmtime/src/**/*.rs`

## `Config`

The caps and the isolation profile a Frontend asks the engine for.

```rust
pub struct Config {}
```

## `Driver`

The bundled engine over one Guest Binary, answering the engine seams.

```rust
pub struct Driver {}
```

## `Driver::new`

Load a Guest Binary into the engine under the given caps, refusing one that
cannot be loaded.

```rust
impl Driver {
    pub fn new(path: &Path, config: Config) -> Result<Self, SetupError> {}
}
```
