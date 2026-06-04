# kobako-core

Guest ABI contract crate for kobako Guest Binaries — the
language-agnostic half of [kobako](https://github.com/elct9620/kobako),
an in-process Wasm sandbox for running untrusted mruby scripts from
Ruby.

A kobako Guest Binary is any `wasm32-wasip1` module implementing the
kobako Guest ABI; the bundled guest embeds mruby, but conformance is
the ABI, not the interpreter. This crate turns that ABI into a
compiler-checked contract:

- `Guest` trait + `export_guest!` — the export enumeration as a trait;
  the macro emits every `#[no_mangle]` export in the invoking crate
- `codec` — the MessagePack wire codec (12-type set + 3 ext types),
  byte-for-byte symmetric with the host gem's Ruby codec
- `transport` — Request / Response / Yield envelopes plus the dispatch
  proxy to the host
- `outcome` — the per-invocation Result / Panic envelopes
- `abi` / `frames` — outcome buffer, packed-u64 helpers, stdin frame
  reader, and `ABI_VERSION`

## Usage

```toml
[lib]
crate-type = ["cdylib"]

[dependencies]
kobako-core = "0.1"
```

```rust
use kobako_core::Guest;

struct MyGuest;

impl Guest for MyGuest {
    fn eval() { /* run one invocation, write the outcome */ }
    fn run(env: &[u8]) { /* entrypoint dispatch */ }
    // yield_to_block keeps its trapping default for guests
    // without block support
}

kobako_core::export_guest!(MyGuest);
```

The host gem loads any conforming Guest Binary:

```ruby
sandbox = Kobako::Sandbox.new(wasm_path: "path/to/my_guest.wasm")
```

## Contract

Behavior contracts live in the repository's
[SPEC.md](https://github.com/elct9620/kobako/blob/main/SPEC.md); the
byte-level wire and ABI signatures in
[docs/wire-codec.md](https://github.com/elct9620/kobako/blob/main/docs/wire-codec.md).
The crate reports `abi::ABI_VERSION` through the macro-emitted
`__kobako_abi_version` export; the host validates it by equality at
Sandbox construction and rejects skew with `Kobako::SetupError`.

## License

Apache-2.0
