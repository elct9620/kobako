# MessagePack dialect

What a schema author or an overlay builds on when it speaks the MessagePack
dialect. It covers the values the dialect carries and the reader and writer for
them.
Which values both peers carry is the payload peers' to hold.

## Includes

- `crates/kobako-codec/src/**/*.rs`

## `Value`

One value of the dialect, restricted to the types the Codec carries.

```rust
pub enum Value {}
```

## `Encoder`

Writes values into the dialect's bytes.

```rust
pub struct Encoder {}
```

## `Encoder::new`

A writer with nothing written yet.

```rust
impl Encoder {
    pub fn new() -> Self {}
}
```

## `Encoder::encode`

Write one value to its bytes.

```rust
impl Encoder {
    pub fn encode(value: &Value) -> Result<Vec<u8>, Error> {}
}
```

## `Encoder::write_value`

Append one value to what is written so far.

```rust
impl Encoder {
    pub fn write_value(&mut self, value: &Value) -> Result<(), Error> {}
}
```

## `Encoder::into_bytes`

The bytes written so far.

```rust
impl Encoder {
    pub fn into_bytes(self) -> Vec<u8> {}
}
```

## `Decoder`

Reads values back out of the dialect's bytes.

```rust
pub struct Decoder {}
```

## `Decoder::new`

A reader at the start of the given bytes.

```rust
impl<'a> Decoder<'a> {
    pub fn new(input: &'a [u8]) -> Self {}
}
```

## `Decoder::position`

How far into the bytes the reader is.

```rust
impl<'a> Decoder<'a> {
    pub fn position(&self) -> usize {}
}
```

## `Decoder::at_end`

Whether every byte has been read.

```rust
impl<'a> Decoder<'a> {
    pub fn at_end(&self) -> bool {}
}
```

## `Decoder::read_value`

Read the next value.

```rust
impl<'a> Decoder<'a> {
    pub fn read_value(&mut self) -> Result<Value, Error> {}
}
```

## `Decoder::read_only_value`

Read the one value the bytes hold, refusing any bytes after it.

```rust
impl<'a> Decoder<'a> {
    pub fn read_only_value(&mut self) -> Result<Value, Error> {}
}
```

## `Arguments::new`

A Run's or a Call's positional and keyword arguments, kept apart.

```rust
impl Arguments {
    pub fn new(args: Vec<Value>, kwargs: Vec<(String, Value)>) -> Self {}
}
```

## `MAX_NESTING_DEPTH`

The deepest a value may nest, shared by every walk over the dialect so the
peers refuse the same boundary.

```rust
pub const MAX_NESTING_DEPTH: usize;
```
