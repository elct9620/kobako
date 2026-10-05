# Roadmap

kobako is a Ruby gem providing an in-process Wasm sandbox for untrusted mruby
code: a wasmtime host runs a precompiled `kobako.wasm` guest, with host↔guest
Transport over a MessagePack wire. Features cover one-shot `#eval`, preload +
`#run` dispatch, Service injection at constant-path names, opaque
Capability Handles, block yield re-entry, three-class error attribution,
output capture, a warm Sandbox pool, Extension installation, and
host-parallel execution.

| Feature | Entry Points | Notes |
|---------|-------------|-------|
| ✅ [Sandbox instantiation](docs/spec/behavior/sandbox.md) | [lib/kobako/sandbox.rb](lib/kobako/sandbox.rb) | — |
| ✅ [Service binding](docs/spec/behavior/services.md) | [lib/kobako/catalog/services.rb](lib/kobako/catalog/services.rb) | — |
| ✅ [Synchronous mruby source execution (`#eval`)](docs/spec/behavior/sandbox.md) | [lib/kobako/sandbox.rb](lib/kobako/sandbox.rb) | — |
| ✅ [Guest-initiated Transport dispatch](docs/spec/behavior/transport-dispatch.md) | [lib/kobako/transport/dispatcher.rb](lib/kobako/transport/dispatcher.rb) | — |
| ✅ [Capability Handle encoding and referencing](docs/spec/behavior/transport-dispatch.md) | [lib/kobako/catalog/handles.rb](lib/kobako/catalog/handles.rb) | — |
| ✅ [Three-class error attribution and raising](docs/spec/behavior/outcome.md) | [lib/kobako/outcome.rb](lib/kobako/outcome.rb) | A guest-entry decode failure is exercised only through its payload half; a Run envelope that does not frame is not reachable through the public API |
| ✅ [Guest output capture](docs/spec/behavior/sandbox.md) | [lib/kobako/capture.rb](lib/kobako/capture.rb) | — |
| ✅ [Host–guest message codec](docs/wire-codec.md) | [crates/kobako-transport/](crates/kobako-transport/) (core envelope + ABI, one implementation), [lib/kobako/codec/](lib/kobako/codec/) (host payload codec) | The payload layer has a second implementation in `crates/kobako-codec`; the envelope layer is pinned by golden vectors instead |
| ✅ [Reproducible build pipeline](SPEC.md#code-organization) | [tasks/wasm/build.rake](tasks/wasm/build.rake) | Verified by build-time gates (double-bake byte-identity, gemspec whitelist), not `test/` |
| ✅ [Multi-layer test and benchmark suite](SPEC.md#testing-style) | [test/](test/) | Benchmarks live in [benchmark/](benchmark/) with the gate in `tasks/bench/`; the anchor baseline advances only by deliberate re-bless |
| ✅ [Guest block reception and yield re-entry](docs/spec/behavior/transport-yield.md) | [lib/kobako/transport/yielder.rb](lib/kobako/transport/yielder.rb) | — |
| ✅ [Snippet preloading (`#preload`)](docs/spec/behavior/sandbox.md) | [lib/kobako/catalog/snippets.rb](lib/kobako/catalog/snippets.rb) | — |
| ✅ [Synchronous entrypoint dispatch (`#run`)](docs/spec/behavior/sandbox.md) | [lib/kobako/sandbox.rb](lib/kobako/sandbox.rb) | — |
| ✅ [Warm Sandbox pool checkout (`Kobako::Pool`)](docs/spec/behavior/pool.md) | [lib/kobako/pool.rb](lib/kobako/pool.rb) | — |
| ✅ [Extension installation (`Sandbox#install`)](docs/spec/behavior/extension.md) | [lib/kobako/extension.rb](lib/kobako/extension.rb), [lib/kobako/catalog/extensions.rb](lib/kobako/catalog/extensions.rb) | kobako ships the contract only, no concrete Extension ([docs/extensions.md](docs/extensions.md)) |
| ✅ [Host-parallel execution (`gvl:`)](docs/spec/behavior/runtime.md) | [ext/kobako/src/runtime/gvl.rs](ext/kobako/src/runtime/gvl.rs) | — |
