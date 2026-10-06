# Engine seams

What a third party implements to put its own wasm engine behind a Frontend, and
the values that engine hands back for one Invocation. Both frontends drive the
Guest through these, so an engine answering them serves either.

## Includes

- `crates/kobako-runtime/src/**/*.rs`

## `Runtime`

A wasm engine that runs one Invocation of a Guest Binary and declares the
isolation it provides.

```rust
pub trait Runtime {}
```

## `Runtime::invoke`

Run one Invocation through the given entry, routing each Call to the handler.

```rust
pub trait Runtime {
    fn invoke(&self, entry: Entry<'_>, frames: Frames<'_>, handler: Option<Arc<dyn DispatchHandler>>) -> Result<Snapshot, InvokeError>;
}
```

## `Runtime::profile`

The isolation the engine provides, which a Frontend weighs against the floor
its Host App asked for.

```rust
pub trait Runtime {
    fn profile(&self) -> Profile;
}
```

## `DispatchHandler`

The Frontend's side of a Call: the engine hands it each Call and relays the
Reply back to the Guest.

```rust
pub trait DispatchHandler {}
```

## `DispatchHandler::dispatch`

Answer one Call, with a Yielder for the Block that rode along.

```rust
pub trait DispatchHandler {
    fn dispatch(&self, call: Call<'_>, yielder: &mut dyn Yielder) -> Option<Reply>;
}
```

## `Yielder`

The engine's way back into the Guest while a Call is out, so the host can run
the Block that rode along.

```rust
pub trait Yielder {}
```

## `Yielder::yield_to_block`

Run the Block once with the given arguments and return its Reply.

```rust
pub trait Yielder {
    fn yield_to_block(&mut self, args: &[u8]) -> Result<Vec<u8>, Trap>;
}
```

## `Entry`

Which invocation export an Invocation enters through.

```rust
pub enum Entry {}
```

## `Frames`

The registrations and Snippets an Invocation replays before its entry runs.

```rust
pub struct Frames {}
```

## `Snapshot`

What one Invocation left behind, whether it ended in an Outcome or a Trap.

```rust
pub struct Snapshot {}
```

## `Completion`

How an Invocation ended: the Guest's final message, or the engine stopping it.

```rust
pub enum Completion {}
```

## `Capture`

One captured output channel and whether it reached its cap.

```rust
pub struct Capture {}
```

## `Usage`

What one Invocation consumed of its time and memory.

```rust
pub struct Usage {}
```

## `Profile`

The isolation posture an engine declares, ordered so a floor can be compared
against it.

```rust
pub enum Profile {}
```

## `Trap`

Why the engine stopped the Guest.

```rust
pub enum Trap {}
```

## `SetupError`

Why an Invocation could not start before the engine ran.

```rust
pub enum SetupError {}
```

## `InvokeError`

Why an Invocation produced no Snapshot: a Trap, or a setup that failed first.

```rust
pub enum InvokeError {}
```
