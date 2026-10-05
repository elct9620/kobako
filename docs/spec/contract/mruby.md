# mruby Guest seams

What a third party reaches when it builds its own mruby Guest: an invocation
flow it writes in place of the bundled one, a Codec it supplies in place of
MessagePack, and a capability gem whose methods reach the host.

## Includes

- `wasm/kobako-mruby/src/**/*.rs`

## `Kobako`

The handle to the class registrations that an invocation flow and a Codec both
work through.

```rust
pub struct Kobako {}
```

## `Kobako::init`

Install kobako's own classes on a booted VM, then the gems the Guest composes,
and return the registrations of kobako's classes.

```rust
impl Kobako {
    pub fn init<G: crate::MrbGuest>(mrb: &Mrb) -> Result<Self, beni::Error> {}
}
```

## `Kobako::resolve_raw`

Recover the registrations from a VM, which the caller guarantees `Kobako::init`
already prepared.

```rust
impl Kobako {
    pub unsafe fn resolve_raw(mrb: &Mrb) -> Self {}
}
```

## `Kobako::install_bindings`

Make each bound Service path reachable as a constant in guest code.

```rust
impl Kobako {
    pub fn install_bindings(&self, paths: &[String]) -> Result<(), InstallError> {}
}
```

## `InstallError`

Why a bound Service path could not be installed.

```rust
pub enum InstallError {}
```

## `Kobako::transport_error`

The wire-level failure a guest method's body hands back, raised at the guest
call site.

```rust
impl Kobako {
    pub fn transport_error(&self, msg: &str) -> beni::Error {}
}
```

## `Kobako::extract_backtrace`

The backtrace a guest exception carries, as its lines.

```rust
impl Kobako {
    pub fn extract_backtrace(&self, exc_val: Value) -> Vec<String> {}
}
```

## `Kobako::top_level_constants`

The names defined at the top level of guest code.

```rust
impl Kobako {
    pub fn top_level_constants(&self) -> Vec<String> {}
}
```

## `Kobako::set_handle_id`

Write the id a guest Handle stands for.

```rust
impl Kobako {
    pub fn set_handle_id(&self, target: Value, id_val: Value) -> Result<(), beni::Error> {}
}
```

## `Kobako::extract_handle_id`

Read the id a guest Handle stands for.

```rust
impl Kobako {
    pub fn extract_handle_id(&self, handle_val: Value) -> u32 {}
}
```

## `Kobako::mint_handle`

Build the guest Handle that stands for an id.

```rust
impl Kobako {
    pub fn mint_handle(&self, id: u32) -> Value {}
}
```

## `Kobako::narrow_int`

Build a guest Integer from a wire integer, refusing one the guest cannot hold.

```rust
impl Kobako {
    pub fn narrow_int<N>(&self, n: N) -> Result<Value, IntegerOutOfRange> {}
}
```

## `IntegerOutOfRange`

A wire integer outside what a guest Integer holds.

```rust
pub struct IntegerOutOfRange(pub i128);
```

## `IntegerOutOfRange::message`

The message naming the integer the guest could not hold.

```rust
impl IntegerOutOfRange {
    pub fn message(self) -> String {}
}
```

## `PayloadCodec`

The Codec a Guest names for every payload an Envelope carries: the
Outcome, a Call and its Reply, a Run, and a Yield.

```rust
pub trait PayloadCodec {}
```

## `Arguments`

A Run's positional and keyword arguments, kept apart, as the Entrypoint is called
with them.

```rust
pub struct Arguments {}
```

## `CodecError`

Why a Codec could not do its part: a value with no form, bytes it cannot read, a
position it does not serve, or the interpreter refusing.

```rust
pub enum CodecError {}
```

## `CodecError::unrepresentable`

Refuse a guest value the Codec has no form for.

```rust
impl CodecError {
    pub fn unrepresentable(kobako: &Kobako, value: beni::Value) -> Self {}
}
```

## `dispatch`

Send a Call to the host and wait for its Reply, keeping the method's block
reachable by the host's yields while the Call is out.

```rust
pub fn dispatch(target: Target<'_>, method: &str, block: Option<Proc>, payload: &[u8]) -> Result<Vec<u8>, DispatchError> {}
```
