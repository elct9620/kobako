# Guest ABI

What a Guest of its own implements and calls to answer the host. The bundled
export macro writes the exports into the Guest's own crate. So registered here
is the trait it drives and what that expansion reaches.

## Includes

- `wasm/kobako-core/src/**/*.rs`

## `serve`

Answer one downcall: the entry its tag names, or unsupported for a tag the
Guest does not serve.

```rust
pub fn serve<G: Guest>(request: &[u8]) -> Vec<u8> {}
```

## `answer`

Hand the host an encoded answer, packed as its place and length in the Guest
Binary's memory.

```rust
pub fn answer(bytes: &[u8]) -> u64 {}
```

## `Guest`

A Guest Binary's invocation surface: the entry points the bundled macro's
downcall export serves.

```rust
pub trait Guest {}
```

## `Guest::eval`

Run one invocation of a script and answer with its Outcome.

```rust
pub trait Guest {
    fn eval(frames: Frames, source: Vec<u8>) -> Outcome;
}
```

## `Guest::run`

Call the Entrypoint the host's Run names and answer with its Outcome.

```rust
pub trait Guest {
    fn run(frames: Frames, run: Run) -> Outcome;
}
```

## `Guest::yield_to_block`

Re-enter the guest to run a block for the host, answering with its Reply.

```rust
pub trait Guest {
    fn yield_to_block(_req: &[u8]) -> YieldReply;
}
```

## `DispatchError`

Why a Call came back without a value: the Service's own Fault, or an exchange
that did not complete.

```rust
pub enum DispatchError {}
```

## `alloc`

Hand the host a buffer in the Guest Binary's memory to write a request into.

```rust
pub fn alloc(size: u32) -> u32 {}
```
