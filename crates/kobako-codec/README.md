# kobako-codec

The payload codecs for [kobako](https://github.com/elct9620/kobako), an
in-process Wasm sandbox for running untrusted mruby scripts from Ruby.

kobako's wire has two layers. The core envelope routes a message and
attributes its outcome — that is the fixed half, and it lives in
[kobako-transport](https://crates.io/crates/kobako-transport). What
rides inside an envelope's opaque `payload` field is the replaceable
half, and this crate holds it, one namespace per schema:

- `msgpack` — the default schema, on by default
  - `msgpack::codec` — the MessagePack wire codec (a closed 11-entry
    type set, two of them ext codes), byte-for-byte symmetric with the
    host gem's independent Ruby codec
  - `msgpack::payload` — the `[args, kwargs]` shape a Call or a Run
    payload carries

Two endpoints that agree on another schema carry none of this crate; the
envelope, the ABI, and the version are unchanged by that substitution.
Free of mruby, wasmtime, and any guest-bound ABI, so it compiles on
every target.

## Usage

```toml
[dependencies]
kobako-codec = "0.14.0" # x-release-please-version
```

```rust
use kobako_codec::msgpack::codec::{Decode, Encode, Value};
use kobako_codec::msgpack::payload::Arguments;

let arguments = Arguments {
    args: vec![Value::Int(42)],
    kwargs: vec![("force".to_string(), Value::Bool(true))],
};
let payload = arguments.encode()?;
let decoded = Arguments::decode(&payload_from_the_wire)?;
```

## Contract

Behavior contracts live in the repository's
[SPEC.md](https://github.com/elct9620/kobako/blob/main/SPEC.md); the
byte-level payload format in
[docs/wire/payload-msgpack.md](https://github.com/elct9620/kobako/blob/main/docs/wire/payload-msgpack.md),
and what a replacement codec owes in
[docs/customization.md](https://github.com/elct9620/kobako/blob/main/docs/customization.md).
Consistency with the host gem's independent Ruby implementation is
established by bidirectional round-trip fuzz in the kobako repository.

## License

Apache-2.0
