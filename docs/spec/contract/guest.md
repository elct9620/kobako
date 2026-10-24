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
    fn eval(preamble: Preamble, source: Vec<u8>) -> Outcome;
}
```

## `Guest::run`

Call the Entrypoint the host's Run names and answer with its Outcome.

```rust
pub trait Guest {
    fn run(preamble: Preamble, run: Run) -> Outcome;
}
```

## `Guest::yield_to_block`

Re-enter the guest to run the block of the open Call `frame` names, answering with its Reply.

```rust
pub trait Guest {
    fn yield_to_block(_frame: u32, _args: &[u8]) -> YieldReply;
}
```

## `Guest::compile`

Compile the Preamble's source Snippets without running them, answering their
bytecode. The default answers as unsupported.

```rust
pub trait Guest {
    fn compile(_preamble: Preamble) -> CompileReply;
}
```

## `DispatchError`

Why a Call came back without a value. It names the Service's own Fault, a break,
a host that does not serve it, or an unfinished exchange.

```rust
pub enum DispatchError {}
```

## `alloc`

Hand the host a buffer in the Guest Binary's memory to write a request or an answer into.

```rust
pub fn alloc(size: u32) -> u32 {}
```
