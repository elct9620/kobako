# kobako-codec

The payload codecs for [kobako](https://github.com/elct9620/kobako), an
in-process Wasm sandbox for running untrusted mruby scripts from Ruby.

kobako's wire has two layers. The Envelope routes a message and
attributes its outcome — the fixed half, living in
[kobako-transport](https://crates.io/crates/kobako-transport). What rides
inside an envelope's opaque `payload` field is the replaceable half, and
this crate holds it, one namespace per schema.

| Module | What it carries |
|---|---|
| `msgpack` | the default schema, on by default |
| `msgpack::codec` | the MessagePack wire codec over a closed 11-entry type set |
| `msgpack::payload` | the `[args, kwargs]` shape a Call or a Run payload carries |

Two of the codec's 11 types are ext codes. The codec is byte-for-byte
symmetric with the host gem's independent Ruby codec, which bidirectional
round-trip fuzz in the kobako repository establishes.

Two endpoints that agree on another schema carry none of this crate; the
envelope, the ABI, and the version are unchanged by that substitution.
Free of mruby, wasmtime, and any guest-bound ABI, so it compiles on
every target.

## Usage

Add the crate, then encode and decode a payload through its schema.

```toml
[dependencies]
kobako-codec = "0.21.0" # x-release-please-version
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

## License

Licensed under [Apache-2.0](https://www.apache.org/licenses/LICENSE-2.0).
