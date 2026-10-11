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
| `Guest` trait + `export_guest!` | the operations as a trait; the macro emits each `#[no_mangle]` export |
| `proxy` / `dispatch` | the guest dispatch path to the host over `__kobako_upcall` |
| `abi` | `serve` behind the downcall export, `alloc`, and `ABI_VERSION` |

The messages themselves live in
[kobako-transport](https://crates.io/crates/kobako-transport): the
envelopes, the Outcome records, and the ABI's own values. It is this
crate's only dependency and the tier every kobako assembly shares. The
messages a `Guest` takes and answers with — `Preamble`, `Flag`, `Outcome`,
`Run`, `Unsupported`, `YieldReply` and `CompileReply` — are re-exported here. A payload codec is not among them:
this crate routes messages without reading one, so a guest speaking its
own schema builds on it unchanged.

## Usage

```toml
[lib]
crate-type = ["cdylib"]

[dependencies]
kobako-core = "0.17.0" # x-release-please-version
```

```rust
use kobako_core::{Guest, Outcome, Preamble, Run};

struct MyGuest;

impl Guest for MyGuest {
    fn eval(preamble: Preamble, source: Vec<u8>) -> Outcome { /* run the source */ }
    fn run(preamble: Preamble, run: Run) -> Outcome { /* call run.entrypoint */ }
    // yield_to_block keeps its default, which answers a yield as
    // unsupported, for guests without block support
}

kobako_core::export_guest!(MyGuest);
```

The host gem loads any conforming Guest Binary:

```ruby
sandbox = Kobako::Sandbox.new(wasm_path: "path/to/my_guest.wasm")
```

## Contract

The host checks a Guest Binary's ABI version before it runs anything.

| Side | What it does |
|---|---|
| Guest | reports `abi::ABI_VERSION` through `__kobako_abi_version` |
| Host | requires an equal version at Sandbox construction |
| Host, on skew | rejects the Guest Binary with `Kobako::SetupError` |

## License

Licensed under [Apache-2.0](https://github.com/elct9620/kobako/blob/main/LICENSE).
