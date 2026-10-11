# Core envelope and ABI values

What every Frontend and Guest shares, whatever Codec fills a payload. That is
the Envelope each message rides in and the values the Guest ABI fixes. A Frontend
of its own composes these with the engine seams; a Guest of its own answers
them.

## Includes

- `crates/kobako-transport/src/**/*.rs`

## `ABI_VERSION`

The Guest ABI version a host accepts a Guest Binary under.

```rust
pub const ABI_VERSION: u32;
```

## `FRAME_LEN_SIZE`

The width of the length prefix in front of each Frame.

```rust
pub const FRAME_LEN_SIZE: usize;
```

## `MAX_DISPATCH_PAYLOAD`

The largest Envelope one dispatch may carry in either direction.

```rust
pub const MAX_DISPATCH_PAYLOAD: usize;
```

## `MAX_FRAME_LEN`

The largest declared length a reader allocates a Frame for.

```rust
pub const MAX_FRAME_LEN: usize;
```

## `pack_ptr_len`

Pack a guest pointer and length into the one value a downcall or an upcall returns.

```rust
pub fn pack_ptr_len(ptr: u32, len: u32) -> u64 {}
```

## `unpack_ptr_len`

Split a downcall's or an upcall's return value back into its pointer and length.

```rust
pub fn unpack_ptr_len(packed: u64) -> (u32, u32) {}
```

## `DecodeError`

Why bytes could not be read as an Envelope.

```rust
pub struct DecodeError {}
```

## `DecodeError::new`

Name why bytes could not be read.

```rust
impl DecodeError {
    pub const fn new(reason: &'static str) -> Self {}
}
```

## `DecodeError::message`

The reason the bytes could not be read.

```rust
impl DecodeError {
    pub fn message(&self) -> &str {}
}
```

## `Call`

A Guest's request that the host invoke a method on a Service or a Handle.

```rust
pub struct Call {}
```

## `Call::decode`

Read a Call from its Envelope.

```rust
impl Call {
    pub fn decode(bytes: &'a [u8]) -> Result<Self, DecodeError> {}
}
```

## `Call::encode`

Write a Call into its Envelope.

```rust
impl Call {
    pub fn encode(&self) -> Vec<u8> {}
}
```

## `Target`

What a Call is addressed to: a Service path or a Handle.

```rust
pub enum Target {}
```

## `Downcall`

A request the host sends the guest, under the tag of its operation.

```rust
pub enum Downcall {}
```

## `Downcall::decode`

Read a request the host sent; a tag the registry has not assigned keeps its body.

```rust
impl Downcall {
    pub fn decode(bytes: &[u8]) -> Result<Self, DecodeError> {}
}
```

## `Downcall::encode`

Write a request under its operation's tag.

```rust
impl Downcall {
    pub fn encode(&self) -> Vec<u8> {}
}
```

## `Upcall`

A request the guest sends the host, under the tag of its operation.

```rust
pub enum Upcall {}
```

## `Upcall::decode`

Read a request the guest sent; a tag the registry has not assigned keeps its body.

```rust
impl Upcall {
    pub fn decode(bytes: &'a [u8]) -> Result<Self, DecodeError> {}
}
```

## `Upcall::encode`

Write a request under its operation's tag.

```rust
impl Upcall {
    pub fn encode(&self) -> Vec<u8> {}
}
```

## `UnknownTag`

A request tag the registry has not assigned, the only tag an unknown request can carry.

```rust
pub struct UnknownTag {}
```

## `UnknownTag::new`

Take a tag as unknown, or refuse one the registry assigns.

```rust
impl UnknownTag {
    pub fn new(tag: u8) -> Option<Self> {}
}
```

## `UnknownTag::get`

The tag's byte.

```rust
impl UnknownTag {
    pub fn get(self) -> u8 {}
}
```

## `Unsupported`

A receiver's answer that it does not serve the operation it was sent.

```rust
pub struct Unsupported {}
```

## `Unsupported::new`

A refusal carrying a description.

```rust
impl Unsupported {
    pub fn new(message: impl Into<String>) -> Self {}
}
```

## `Unsupported::encode`

Write the refusal as a whole answer, the same for every operation.

```rust
impl Unsupported {
    pub fn encode(&self) -> Vec<u8> {}
}
```

## `Reply`

The host's answer to a Call: a value, a Fault, or that it does not serve it.

```rust
pub enum Reply {}
```

## `Reply::decode`

Read a Reply from its Envelope.

```rust
impl Reply {
    pub fn decode(bytes: &[u8]) -> Result<Self, DecodeError> {}
}
```

## `Reply::encode`

Write a Reply into its Envelope.

```rust
impl Reply {
    pub fn encode(&self) -> Vec<u8> {}
}
```

## `YieldReply`

How one yield into the block ended.

```rust
pub enum YieldReply {}
```

## `YieldReply::decode`

Read a yield's answer from its Envelope.

```rust
impl YieldReply {
    pub fn decode(bytes: &[u8]) -> Result<Self, DecodeError> {}
}
```

## `YieldReply::encode`

Write a yield's answer into its Envelope.

```rust
impl YieldReply {
    pub fn encode(&self) -> Vec<u8> {}
}
```

## `Fault`

A Service failure as the wire carries it: its category and message.

```rust
pub struct Fault {}
```

## `Fault::new`

A Fault of the given category.

```rust
impl Fault {
    pub fn new(kind: FaultKind, message: impl Into<String>) -> Self {}
}
```

## `FaultKind`

The category of a Service failure, which each Frontend maps onto its own error.

```rust
pub enum FaultKind {}
```

## `FaultKind::name`

The name a Frontend uses for the category.

```rust
impl FaultKind {
    pub fn name(self) -> &'static str {}
}
```

## `FaultKind::from_name`

Read a category from the name a Frontend uses.

```rust
impl FaultKind {
    pub fn from_name(name: &str) -> Option<Self> {}
}
```

## `Outcome`

How an invocation ended: its value, a Panic, or that the guest does not serve it.

```rust
pub enum Outcome {}
```

## `Outcome::decode`

Read an Outcome from its Envelope.

```rust
impl Outcome {
    pub fn decode(bytes: &[u8]) -> Result<Self, DecodeError> {}
}
```

## `Outcome::encode`

Write an Outcome into its Envelope.

```rust
impl Outcome {
    pub fn encode(&self) -> Vec<u8> {}
}
```

## `Origin`

Whether a Panic is a Sandbox failure or a Service failure.

```rust
pub enum Origin {}
```

## `Origin::name`

The name the wire carries for the side.

```rust
impl Origin {
    pub fn name(self) -> &'static str {}
}
```

## `Origin::from_name`

Read a side from the name the wire carries.

```rust
impl Origin {
    pub fn from_name(name: &str) -> Self {}
}
```

## `Panic`

An uncaught failure, with what attributing and correcting it needs.

```rust
pub struct Panic {}
```

## `ErrorRecord`

A script error as the wire carries it: its class name, message, and backtrace.

```rust
pub struct ErrorRecord {}
```

## `Run`

The Entrypoint an invocation calls and the payload it is called with.

```rust
pub struct Run {}
```

## `Run::decode`

Read a Run from its Envelope.

```rust
impl Run {
    pub fn decode(bytes: &[u8]) -> Result<Self, DecodeError> {}
}
```

## `Run::encode`

Write a Run into its Envelope.

```rust
impl Run {
    pub fn encode(&self) -> Vec<u8> {}
}
```

## `Preamble`

The bindings, Snippets, and flags an invocation is set up with before its entry runs.

```rust
pub struct Preamble {}
```

## `Preamble::decode`

Read a Preamble from its record list.

```rust
impl Preamble {
    pub fn decode(bytes: &[u8]) -> Result<Self, DecodeError> {}
}
```

## `Preamble::encode`

Write a Preamble as its record list.

```rust
impl Preamble {
    pub fn encode(&self) -> Vec<u8> {}
}
```

## `Bindings`

The Service paths a Sandbox registers, as the Preamble carries them.

```rust
pub struct Bindings {}
```

## `Snippet`

One Snippet an invocation replays before its entry runs.

```rust
pub enum Snippet {}
```

## `Snippets`

The Snippets an invocation replays, in order.

```rust
pub struct Snippets {}
```

## `Flag`

One switch the host sets for an invocation, with whether a Guest Binary may skip it.

```rust
pub struct Flag {}
```
