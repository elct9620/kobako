# Roadmap

kobako is a Ruby gem providing an in-process Wasm sandbox for untrusted mruby
code: a wasmtime host runs a precompiled `kobako.wasm` guest, with host↔guest
Transport over a MessagePack wire. Features cover one-shot `#eval`, preload +
`#run` dispatch, Service injection at constant-path names, opaque
Capability Handles, block yield re-entry, three-class error attribution,
output capture, and a warm Sandbox pool.

| Feature | Entry Points | Notes |
|---------|-------------|-------|
| ✅ [F-01 Sandbox instantiation](docs/spec/behavior/sandbox.md) | [lib/kobako/sandbox.rb](lib/kobako/sandbox.rb) | — |
| ✅ [F-02 Service binding](docs/spec/behavior/services.md) | [lib/kobako/catalog/services.rb](lib/kobako/catalog/services.rb) | — |
| ✅ [F-04 Synchronous mruby source execution (`#eval`)](docs/spec/behavior/sandbox.md) | [lib/kobako/sandbox.rb](lib/kobako/sandbox.rb) | — |
| ✅ [F-05 Guest-initiated Transport dispatch](docs/spec/behavior/transport-dispatch.md) | [lib/kobako/transport/dispatcher.rb](lib/kobako/transport/dispatcher.rb) | — |
| ✅ [F-06 Capability Handle encoding and referencing](docs/spec/behavior/transport-dispatch.md) | [lib/kobako/catalog/handles.rb](lib/kobako/catalog/handles.rb) | — |
| ✅ [F-07 Three-class error attribution and raising](docs/spec/behavior/outcome.md) | [lib/kobako/outcome.rb](lib/kobako/outcome.rb) | A guest-entry decode failure is exercised only through its payload half ([`CD-003`](docs/spec/behavior/codec.md)); a Run envelope that does not frame is not reachable through the public API |
| ✅ [F-08 Guest output capture](docs/spec/behavior/sandbox.md) | [lib/kobako/capture.rb](lib/kobako/capture.rb) | — |
| ✅ [F-09 Host–guest message codec](docs/wire-codec.md) | [crates/kobako-transport/](crates/kobako-transport/) (core envelope + ABI, one implementation), [lib/kobako/codec/](lib/kobako/codec/) (host payload codec) | The payload layer has a second implementation in `crates/kobako-codec`; the envelope layer is pinned by golden vectors instead |
| ✅ [F-10 Reproducible build pipeline](SPEC.md#code-organization) | [tasks/wasm/build.rake](tasks/wasm/build.rake) | Verified by build-time gates (`rake anchors`, double-bake byte-identity, gemspec whitelist), not `test/` |
| ✅ [F-11 Multi-layer test and benchmark suite](SPEC.md#testing-style) | [test/](test/) | Benchmarks live in [benchmark/](benchmark/) with the gate in `tasks/bench/`; the anchor baseline advances only by deliberate re-bless |
| ✅ [F-12 Guest block reception and yield re-entry](docs/spec/behavior/transport-yield.md) | [lib/kobako/transport/yielder.rb](lib/kobako/transport/yielder.rb) | — |
| ✅ [F-13 Snippet preloading (`#preload`)](docs/spec/behavior/sandbox.md) | [lib/kobako/catalog/snippets.rb](lib/kobako/catalog/snippets.rb) | — |
| ✅ [F-14 Synchronous entrypoint dispatch (`#run`)](docs/spec/behavior/sandbox.md) | [lib/kobako/sandbox.rb](lib/kobako/sandbox.rb) | — |
| ✅ [F-15 Warm Sandbox pool checkout (`Kobako::Pool`)](docs/spec/behavior/pool.md) | [lib/kobako/pool.rb](lib/kobako/pool.rb) | — |
