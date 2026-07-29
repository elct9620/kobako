# Fixed Schema (a guest and a host that agree on protobuf)

A complete kobako assembly — its own Guest Binary and its own host — where every payload is a protobuf message instead of the bundled MessagePack one. Where [`plugin-rs`](../plugin-rs) shows the SDK's conveniences and [`wire-rs`](../wire-rs) shows the wire underneath them, this one replaces something both of those keep: the schema. Neither half links a payload codec it did not write.

The interesting part is where a fixed schema attaches. A payload codec (`MrbGuest::Codec`) is handed arguments with no method name, so it can only carry a schema that describes itself. A protobuf message has to be chosen *before* the arguments are read, which means the choice belongs at a call site that already knows which method it is — and a Ruby-visible method defined by a capability gem is exactly that.

```
MyService::KV.get("k")   ->  a real method the gem defined
                         ->  encodes GetRequest, calls proxy::dispatch
                         ->  Call{ target: Path("MyService::KV"), method: "get" }
session.get("k")         ->  the same schema, the same method name
                         ->  Call{ target: Handle(7), method: "get" }
```

The Call envelope carries `target` and `method`, and that pair *is* the schema key — which is why the host below reads the method name before it reads a payload byte, and why a Handle needs no representation of its own: an id is the whole of what crosses.

## What is here

| Piece | What it demonstrates |
|---|---|
| `guest/shell` | Naming the two things a guest shell owns — the payload schema and the gem set — then emitting the ABI exports. Its codec serves the three positions no call site owns, and only one of those carries a schema of its own: the value an invocation ends on. The other two hand off to the gem, and the two that serve the dynamic proxy it leaves unwritten. |
| `guest/kv` | A capability gem that reaches the wire. Real methods encode their own requests; `MyService::Session` is a Handle a script can call methods on but cannot construct; `each_key` takes a block, which the gem holds across its own dispatch so the host can yield into it. |
| `host` | `Receiver` implemented directly, so the payload bytes and the schema are the host's own. `Handles::alloc` issues an id, `Handles::resolve` turns one back into the object, and a detached table makes the whole thing unit-testable without a guest. |
| `host/src/entry.rs` | Entering at a preloaded `App.call(body, env)` instead of with a script, and the two ways a capability arrives: `env` rides *in the request* as a Handle this invocation's table just issued, while `MyService::KV` is filled at its declared path for the invocation. A handler should be handed anything request-scoped — a name that outlives the request is a name the next request could reach. |
| `Rakefile` + `build_config/` | The three build stages any kobako guest takes: beni for `libmruby.a`, cargo for the cdylib, `kobako-baker` for the boot image. |

## Building

The guest half needs a toolchain; the first build fetches one into `vendor/`.

```bash
cd examples/fixed-schema-rs
bundle install
bundle exec rake build      # produces guest.wasm
```

`BENI_VENDOR_DIR` points the build at an existing toolchain tree instead of fetching another. The bake step is optional and skipped when `kobako-baker` is absent — an unbaked artifact boots on its first invocation rather than at build time, which costs time and changes no behaviour.

## Running

```bash
cd host

cargo run --release -- ../guest.wasm             # the demo
cargo run --release -- ../guest.wasm --run       # a preloaded App.call entrypoint
cargo run --release -- ../guest.wasm --check     # the claims, as invocations
cargo run --release -- ../guest.wasm 'MyService::KV.put("a", "1"); "done"'

cargo test                                       # the host half, no guest needed
```

`--check` is where the properties described here are settled — as invocations, because a codec and a guest-side dispatch both need a live interpreter. The host's own half needs no guest: `cargo test` runs the `Receiver` directly against a detached Handle table, which is the shape to reach for when what you are checking is the schema rather than the round trip.

## What a fixed schema gives up

Each of these follows from one decision — that a payload is chosen at a call site that knows its own method — rather than from three separate ones.

| Not available | Why |
|---|---|
| Any Service reached dynamically | The codec does not serve the dispatch-argument position, so `method_missing` never reaches the wire. A method exists because a gem defined it, or it does not exist. Reaching for one raises `NotImplementedError` — a capability this sandbox lacks, not a message it could not read, and under `ScriptError` so a bare `rescue` does not quietly turn it into a value. |
| `Kobako::Extension` backends | An idiom's privileged methods fall through to its bound backend dynamically, which is that same refusal. Under a static design a `beni::Gem` covers the same ground and covers it better, with the idiom compiled rather than replayed. |
| More than one shape per context-free position | An outcome, a `#run` payload, and a yield are each handed to the codec with no name attached, so each has one shape per guest. A gem wanting several can keep its own note of which is in flight — the yield is synchronous inside the dispatch that carried the block — but nothing hands it one. |

## Three things worth knowing

**The outcome's agreement is not on the wire.** This guest's codec carries a byte string out of an invocation and refuses everything else, so a script must end on a String. What those bytes *mean* is settled by the host having written the script — nothing checks it. That holds for a host that owns both sides, and stops holding the moment the script comes from somewhere else.

**A Handle a script can forge stops meaning what it says.** The host is the boundary regardless: an id it never issued is refused there, and the table lives for one invocation, so a guessed id reaches at most an object this same guest was already given. `MyService::Session` refusing `new` and freezing on the way out is defence in depth on top of that, not the boundary itself — but it is what a gem carrying its own Handle representation takes on, and no gate will tell you if you skip it.

**One artifact carries one interpreter.** The gem, the shell, and `kobako-mruby` must resolve to the same semver-compatible `beni`; two of them would be two incompatible `Mrb` types in one build. Nothing announces this beyond the type error you get.

Both halves pin the released crates from crates.io, so this directory builds on its own — the same path a third party takes.
