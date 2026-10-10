# kobako-core

Guest ABI contract crate for kobako Guest Binaries — the
language-agnostic half of [kobako](https://github.com/elct9620/kobako),
an in-process Wasm sandbox for running untrusted mruby scripts from
Ruby.

A kobako Guest Binary is any `wasm32-wasip1` module implementing the
kobako Guest ABI. The bundled guest embeds mruby, but conformance is
the ABI, not the interpreter. This crate turns that ABI into a
compiler-checked contract.

| Item | What it provides |
|---|---|
| `Guest` trait + `export_guest!` | the export set as a trait; the macro emits each `#[no_mangle]` export |
| `proxy` / `dispatch` | the guest dispatch path to the host over `__kobako_dispatch` |
| `abi` / `frames` | outcome buffer, packed-u64 helpers, stdin frame reader, and `ABI_VERSION` |

The messages themselves live in
[kobako-transport](https://crates.io/crates/kobako-transport): the
envelopes, the Outcome records, and the ABI's own values. It is this
crate's only dependency and the tier every kobako assembly shares. A
payload codec is not among them: this crate routes messages without
reading one, so a guest speaking its own schema builds on it unchanged.

## Usage

```toml
[lib]
crate-type = ["cdylib"]

[dependencies]
kobako-core = "0.17.0" # x-release-please-version
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

The crate reports `abi::ABI_VERSION` through the macro-emitted
`__kobako_abi_version` export; the host validates it by equality at
Sandbox construction and rejects skew with `Kobako::SetupError`.

## License

Apache-2.0
