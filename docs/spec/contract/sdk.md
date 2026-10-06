# Rust SDK interface

What a Rust embedder builds a Sandbox with, what it implements to be reached
from the guest, and what it reads off a finished invocation. The Ruby frontend
answers the same behaviors; this registers the shape the Rust caller writes for
them.

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

## `Context`

What an override closure receives to shape one invocation: it overrides what a
path resolves to, for that invocation alone.

```rust
pub struct Context<'a> {}
```

## `YieldError`

How a yield into the guest block ended short of a value, which a Receiver
matches to recover or hands up to stop.

```rust
pub enum YieldError {}
```

## `Sandbox`

One Sandbox: the Guest Binary it loads and the registrations every invocation
starts from.

```rust
pub struct Sandbox {}
```

## `Sandbox::new`

Load a Guest Binary into the bundled engine under the given caps.

```rust
impl Sandbox {
    pub fn new(wasm_path: impl AsRef<Path>, options: Options) -> Result<Self, Error> {}
}
```

## `Sandbox::with_runtime`

Build a Sandbox over an engine the caller brings, refusing one that declares
less isolation than the floor asked for.

```rust
impl Sandbox {
    pub fn with_runtime(runtime: impl Runtime + Send + Sync + 'static, profile: Profile) -> Result<Self, Error> {}
}
```

## `Sandbox::bind`

Bind a Service at a constant path for every invocation.

```rust
impl Sandbox {
    pub fn bind(&mut self, path: &str, object: Arc<dyn Receiver>) -> Result<(), Error> {}
}
```

## `Sandbox::bind_fillable`

Reserve a constant path that each invocation fills through its Context.

```rust
impl Sandbox {
    pub fn bind_fillable(&mut self, path: &str) -> Result<(), Error> {}
}
```

## `Sandbox::install`

Compose an Extension into the Sandbox.

```rust
impl Sandbox {
    pub fn install(&mut self, extension: Arc<dyn Extension>) -> Result<(), Error> {}
}
```

## `Sandbox::preload`

Register a source Snippet every invocation replays.

```rust
impl Sandbox {
    pub fn preload(&mut self, name: &str, source: &str) -> Result<(), Error> {}
}
```

## `Sandbox::preload_binary`

Register a bytecode Snippet every invocation replays.

```rust
impl Sandbox {
    pub fn preload_binary(&mut self, bytecode: impl Into<Vec<u8>>) -> Result<(), Error> {}
}
```

## `Sandbox::eval`

Run a script as one invocation.

```rust
impl Sandbox {
    pub fn eval(&self, source: &str) -> Result<Execution, Error> {}
}
```

## `Sandbox::eval_with`

Run a script as one invocation, shaped first through its Context.

```rust
impl Sandbox {
    pub fn eval_with<F>(&self, source: &str, overrides: F) -> Result<Execution, Error> {}
}
```

## `Sandbox::run`

Call an Entrypoint as one invocation.

```rust
impl Sandbox {
    pub fn run(&self, target: &str, payload: RunPayload<'_>) -> Result<Execution, Error> {}
}
```

## `Sandbox::run_with`

Call an Entrypoint as one invocation, shaped first through its Context.

```rust
impl Sandbox {
    pub fn run_with<F>(&self, target: &str, payload: RunPayload<'_>, overrides: F) -> Result<Execution, Error> {}
}
```

## `Options`

The caps and the isolation floor a Sandbox is built under.

```rust
pub struct Options {}
```

## `Context::bind`

Bind a Service at a path for this invocation alone.

```rust
impl Context<'_> {
    pub fn bind(&mut self, path: &str, object: Arc<dyn Receiver>) -> Result<(), Error> {}
}
```

## `RunPayload`

The arguments one `run` carries, in the payload codec's bytes.

```rust
pub struct RunPayload {}
```

## `RunPayload::bytes`

A payload that is already complete.

```rust
impl<'a> RunPayload<'a> {
    pub fn bytes(bytes: impl Into<Vec<u8>>) -> Self {}
}
```

## `RunPayload::build`

A payload finished once the invocation's Handle table exists, so it can carry
host objects.

```rust
impl<'a> RunPayload<'a> {
    pub fn build(build: impl FnOnce(&Handles<'_>) -> Result<Vec<u8>, Error> + 'a) -> Self {}
}
```

## `Execution`

One finished invocation: its result, its captured output, and what it used.

```rust
pub struct Execution {}
```

## `Execution::resolve`

Recover the host object a Handle in the result stands for.

```rust
impl Execution {
    pub fn resolve(&self, id: u32) -> Option<Arc<dyn Receiver>> {}
}
```

## `Execution::stderr`

What the guest wrote to its second descriptor.

```rust
impl Execution {
    pub fn stderr(&self) -> &[u8] {}
}
```

## `Execution::stdout_truncated`

Whether the stdout cap clipped the output.

```rust
impl Execution {
    pub fn stdout_truncated(&self) -> bool {}
}
```

## `Execution::stderr_truncated`

Whether the stderr cap clipped the output.

```rust
impl Execution {
    pub fn stderr_truncated(&self) -> bool {}
}
```

## `Error`

Why an invocation or a setup step failed, one variant per error class the Ruby
frontend raises.

```rust
pub enum Error {}
```

## `Failure`

One record of a failed invocation: what failed, where in the script, and any
correction it offers.

```rust
pub struct Failure {}
```

## `Receiver`

A host object the guest reaches through a Service path or a Handle.

```rust
pub trait Receiver {}
```

## `Receiver::call`

Answer one method call from the guest, with the Block that rode along.

```rust
pub trait Receiver {
    fn call(&self, method: &str, payload: &[u8], block: Option<&mut Yielder<'_>>, handles: &Handles<'_>) -> Result<Vec<u8>, Fault>;
}
```

## `Receiver::respond_to_guest`

Whether the guest may call the method at all.

```rust
pub trait Receiver {
    fn respond_to_guest(&self, method: &str) -> bool;
}
```

## `Yielder::call_payload`

Call the Block that rode along with a Call once and return its value, in the
payload codec's bytes.

```rust
impl<'y> Yielder<'y> {
    pub fn call_payload(&mut self, args: &[u8]) -> Result<Vec<u8>, YieldError> {}
}
```

## `Detached`

A Handle table with no invocation behind it, so a Receiver written outside this
crate can be called on its own.

```rust
pub struct Detached {}
```

## `Detached::new`

An empty table.

```rust
impl Detached {
    pub fn new() -> Self {}
}
```

## `Detached::as_handles`

The table as the Handles a Receiver takes.

```rust
impl Detached {
    pub fn as_handles(&self) -> Handles<'_> {}
}
```

## `Extension`

A guest idiom paired with an optional Backend, installed into a Sandbox as one
unit.

```rust
pub trait Extension {}
```

## `Extension::name`

The name other Extensions depend on this one by.

```rust
pub trait Extension {
    fn name(&self) -> &str;
}
```

## `Extension::source`

The guest idiom the Extension installs.

```rust
pub trait Extension {
    fn source(&self) -> &str;
}
```

## `Extension::depends_on`

The Extensions that must be installed alongside this one.

```rust
pub trait Extension {
    fn depends_on(&self) -> &[&str];
}
```

## `Extension::backend`

The Backend the Extension binds, if it has one.

```rust
pub trait Extension {
    fn backend(&self) -> Option<Backend>;
}
```

## `Backend`

The host side of an Extension: the path it binds at and how its object is
sourced.

```rust
pub struct Backend {}
```

## `Provider`

How a Backend's object is sourced: once, per invocation, or filled through the
Context.

```rust
pub enum Provider {}
```

## `ValueReceiver`

A Receiver that speaks MessagePack values rather than bytes.

```rust
pub trait ValueReceiver {}
```

## `ValueReceiver::call`

Answer one method call from the guest with decoded arguments.

```rust
pub trait ValueReceiver {
    fn call(&self, method: &str, args: &[Value], kwargs: &[(String, Value)], block: Option<&mut Yielder<'_>>, handles: &Handles<'_>) -> Result<Value, Fault>;
}
```

## `ValueReceiver::respond_to_guest`

Whether the guest may call the method at all.

```rust
pub trait ValueReceiver {
    fn respond_to_guest(&self, method: &str) -> bool;
}
```

## `ValueReceiver::into_receiver`

Wrap the object as a byte-level Receiver a Sandbox binds.

```rust
pub trait ValueReceiver {
    fn into_receiver(self) -> Arc<IntoReceiver<Self>>;
}
```

## `IntoReceiver`

A ValueReceiver standing as a byte-level Receiver.

```rust
pub struct IntoReceiver {}
```

## `RunArg`

One MessagePack argument to `run`: a value, or a host object passed as a Handle.

```rust
pub enum RunArg {}
```

## `RunPayload::values`

A payload built from MessagePack arguments.

```rust
impl RunPayload<'_> {
    pub fn values(args: Vec<RunArg>, kwargs: Vec<(String, RunArg)>) -> Self {}
}
```

## `Execution::value`

The invocation's result as a MessagePack value.

```rust
impl Execution {
    pub fn value(&self) -> Result<Value, Error> {}
}
```

## `Execution::resolve_as`

Recover the ValueReceiver a Handle in the result stands for, as the type that
was bound.

```rust
impl Execution {
    pub fn resolve_as<V: ValueReceiver>(&self, id: u32) -> Option<Arc<V>> {}
}
```

## `Handles::resolve_as`

Recover the ValueReceiver a Handle id stands for, as the type that was bound.

```rust
impl Handles<'_> {
    pub fn resolve_as<V: ValueReceiver>(&self, id: u32) -> Option<Arc<V>> {}
}
```

## `Yielder::call_values`

Call the Block once with MessagePack arguments and return its value.

```rust
impl Yielder<'_> {
    pub fn call_values(&mut self, args: &[Value]) -> Result<Value, YieldError> {}
}
```
