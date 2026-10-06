# Roadmap

kobako runs untrusted mruby code in an in-process Wasm sandbox. A wasmtime
host drives a precompiled `kobako.wasm` guest over a MessagePack wire. Each
row below links a feature to its scenarios and to where its code starts.

| Feature | Entry Points |
|---------|-------------|
| ✅ [Sandbox](docs/spec/behavior/sandbox.md) | [lib/kobako/sandbox.rb](lib/kobako/sandbox.rb) |
| ✅ [Service registration](docs/spec/behavior/services.md) | [lib/kobako/catalog/services.rb](lib/kobako/catalog/services.rb) |
| ✅ [Runtime](docs/spec/behavior/runtime.md) | [lib/kobako/runtime.rb](lib/kobako/runtime.rb) |
| ✅ [Pool](docs/spec/behavior/pool.md) | [lib/kobako/pool.rb](lib/kobako/pool.rb) |
| ✅ [Extension](docs/spec/behavior/extension.md) | [lib/kobako/extension.rb](lib/kobako/extension.rb) |
| ✅ [Outcome attribution](docs/spec/behavior/outcome.md) | [lib/kobako/outcome.rb](lib/kobako/outcome.rb) |
| ✅ [Transport dispatch](docs/spec/behavior/transport-dispatch.md) | [lib/kobako/transport/dispatcher.rb](lib/kobako/transport/dispatcher.rb) |
| ✅ [Yield re-entry](docs/spec/behavior/transport-yield.md) | [lib/kobako/transport/yielder.rb](lib/kobako/transport/yielder.rb) |
| ✅ [Dispatch boundary](docs/spec/behavior/transport-boundary.md) | [lib/kobako/transport/exposure.rb](lib/kobako/transport/exposure.rb) |
| ✅ [Payload encoding](docs/spec/behavior/payload-encoding.md) | [lib/kobako/payload/](lib/kobako/payload/) |
| ✅ [Payload wire](docs/spec/behavior/codec.md) | [lib/kobako/codec/](lib/kobako/codec/) |
| ✅ [Core envelope](docs/spec/behavior/envelope.md) | [crates/kobako-transport/](crates/kobako-transport/) |
| ✅ [mruby guest](docs/spec/behavior/mruby.md) | [wasm/kobako-mruby/](wasm/kobako-mruby/) |
| ✅ [Guest IO](docs/spec/behavior/io.md) | [wasm/kobako-io/](wasm/kobako-io/) |
| ✅ [Guest JSON](docs/spec/behavior/json.md) | [wasm/kobako-json/](wasm/kobako-json/) |
| ✅ [Regexp](docs/spec/behavior/regexp.md) | [wasm/kobako-regexp/src/regexp.rs](wasm/kobako-regexp/src/regexp.rs) |
| ✅ [Regexp over String](docs/spec/behavior/regexp-string.md) | [wasm/kobako-regexp/src/string_ext.rs](wasm/kobako-regexp/src/string_ext.rs) |
| ✅ [MatchData](docs/spec/behavior/regexp-matchdata.md) | [wasm/kobako-regexp/src/matchdata.rs](wasm/kobako-regexp/src/matchdata.rs) |
