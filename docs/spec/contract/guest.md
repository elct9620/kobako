# Guest ABI

What a Guest built with the bundled export macro hands the host. The macro
writes its exports into the Guest's own crate, and registered here is what
that expansion reaches.

## Includes

- `wasm/kobako-core/src/**/*.rs`

## `take_outcome`

Hand the host the finished invocation's Outcome bytes, packed as their place
and length in the Guest Binary's memory.

```rust
pub fn take_outcome() -> u64 {}
```
