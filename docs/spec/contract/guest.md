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

## `read_eval_frames`

Read an eval's Frames and the source between them, or the boot failure that stops it.

```rust
pub fn read_eval_frames() -> Result<(Frames, Vec<u8>), Outcome> {}
```

## `read_frames`

Read a run's Frames, or the boot failure that stops it.

```rust
pub fn read_frames() -> Result<Frames, Outcome> {}
```

## `write_outcome`

Leave the invocation's Outcome for the host to take.

```rust
pub fn write_outcome(outcome: Outcome) {}
```

## `decode_run`

Read the Run the host wrote, or answer with the Panic an unframed one fails under.

```rust
pub fn decode_run(env: &[u8]) -> Result<Run, Outcome> {}
```

## `write_reply`

Frame a block's Yield Reply where the host can read it.

```rust
pub fn write_reply(reply: YieldReply) -> u64 {}
```

## `alloc`

Hand the host a buffer in the Guest Binary's memory to write a Reply into.

```rust
pub fn alloc(size: u32) -> u32 {}
```
