# Rust SDK interface

What a Rust embedder reads off a finished invocation, and how a Receiver
hands the guest a stateful host object. The Ruby frontend answers the same
behaviors; this registers the shape the Rust caller writes for them.

## Includes

- `crates/kobako/src/**/*.rs`

## `Execution::payload`

The invocation's result, still in the payload codec's bytes.

```rust
impl Execution {
    pub fn payload(&self) -> Result<&[u8], &Error> {}
}
```

## `Execution::stdout`

What the guest wrote to its first descriptor.

```rust
impl Execution {
    pub fn stdout(&self) -> &[u8] {}
}
```

## `Execution::usage`

What the invocation consumed.

```rust
impl Execution {
    pub fn usage(&self) -> Usage {}
}
```

## `Handles::alloc`

Bind a host object into the invocation's table and return the id that stands
for it on the wire.

```rust
impl<'a> Handles<'a> {
    pub fn alloc(&self, object: Arc<dyn Receiver>) -> Result<u32, Fault> {}
}
```

## `Handles::resolve`

Recover the live host object a Handle id stands for.

```rust
impl<'a> Handles<'a> {
    pub fn resolve(&self, id: u32) -> Option<Arc<dyn Receiver>> {}
}
```
