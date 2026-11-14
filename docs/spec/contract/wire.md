# Wire-symmetric payload peers

The payload types both Codec implementations carry. Each is registered on both
sides so neither peer can drop or reshape its half alone.

The round-trip fuzz holds what the bytes are. The behavior specification holds
that both peers carry the same set. This holds that each peer still answers for
them.

The guest-side traits stay open, so a schema built on this codec implements
them for its own documents.

## Includes

- `crates/kobako-codec/src/msgpack/codec/decoder.rs`
- `crates/kobako-codec/src/msgpack/codec/encoder.rs`

## `Encode`

A payload document that writes itself to the codec's bytes.

```rust
pub trait Encode {}
```

## `Decode`

A payload document that reads itself back from the codec's bytes.

```rust
pub trait Decode {}
```

