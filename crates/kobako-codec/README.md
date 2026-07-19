# kobako-codec

Portable wire tier for [kobako](https://github.com/elct9620/kobako),
an in-process Wasm sandbox for running untrusted mruby scripts from
Ruby.

Host and guest meet over a MessagePack-based Transport wire; this crate
is its Rust expression, usable on either side of the wasm boundary:

- `codec` — the MessagePack wire codec (12-type set + 3 ext types),
  byte-for-byte symmetric with the host gem's Ruby codec
- `transport` — the Request / Response / Yield envelope value objects
- `outcome` — the per-invocation Outcome / Panic envelopes
- `FRAME_LEN_SIZE` — the length-prefix width shared by stdin frames and
  the outcome buffer

The guest-ABI contract crate
([kobako-core](https://crates.io/crates/kobako-core)) builds its
transport machinery on this tier; a Rust host embedding the sandbox
encodes the same envelopes with it directly. The crate is free of
mruby, wasmtime, and any guest-bound ABI, so it compiles on every
target.

## Usage

```toml
[dependencies]
kobako-codec = "0.11.0" # x-release-please-version
```

```rust
use kobako_codec::codec::{Decode, Encode};
use kobako_codec::transport::{Request, Response, Target};

let request = Request {
    target: Target::Path("MyService::KV".into()),
    method: "fetch".into(),
    args: Vec::new(),
    kwargs: Vec::new(),
    block_given: false,
};
let bytes = request.encode()?;
let response = Response::decode(&bytes_from_the_wire)?;
```

## Contract

Behavior contracts live in the repository's
[SPEC.md](https://github.com/elct9620/kobako/blob/main/SPEC.md); the
byte-level wire in
[docs/wire-codec.md](https://github.com/elct9620/kobako/blob/main/docs/wire-codec.md).
Consistency with the host gem's independent Ruby implementation is
established by bidirectional round-trip fuzz in the kobako repository.

## License

Apache-2.0
