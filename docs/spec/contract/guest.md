# Guest ABI

What a Guest of its own implements and calls to answer the host. The bundled
export macro writes the exports into the Guest's own crate. So registered here
is the trait it drives and what that expansion reaches.

## Includes

- `wasm/kobako-core/src/**/*.rs`

## `take_outcome`

Hand the host the finished invocation's Outcome bytes, packed as their place
and length in the Guest Binary's memory.

```rust
pub fn take_outcome() -> u64 {}
```

## `Guest`

A Guest Binary's invocation surface: the entry points behind the exports the
bundled macro writes.

```rust
pub trait Guest {}
```

## `Guest::eval`

Run one invocation of a script and write its Outcome.

```rust
pub trait Guest {
    fn eval();
}
```

## `Guest::run`

Call one Entrypoint from the Run the host wrote and write its Outcome.

```rust
pub trait Guest {
    fn run(env: &[u8]);
}
```

## `Guest::yield_to_block`

Re-enter the guest to run a block for the host, answering with its Reply.

```rust
pub trait Guest {
    fn yield_to_block(_req: &[u8]) -> u64;
}
```

## `DispatchError`

Why a Call came back without a value: the Service's own Fault, or an exchange
that did not complete.

```rust
pub enum DispatchError {}
```

## `read_frame`

Read the next Frame the host wrote to the Guest's input.

```rust
pub fn read_frame() -> Option<Vec<u8>> {}
```

## `write_outcome`

Leave the invocation's Outcome for the host to take.

```rust
pub fn write_outcome(bytes: Vec<u8>) {}
```

## `write_panic`

Leave a Panic as the invocation's Outcome.

```rust
pub fn write_panic(panic: Panic) {}
```

## `alloc`

Hand the host a buffer in the Guest Binary's memory to write a Reply into.

```rust
pub fn alloc(size: u32) -> u32 {}
```
