# Roadmap

kobako is a Ruby gem providing an in-process Wasm sandbox for untrusted mruby
code: a wasmtime host runs a precompiled `kobako.wasm` guest, with host↔guest
Transport over a MessagePack wire. Features cover one-shot `#eval`, preload +
`#run` dispatch, Service injection through Namespaces / Members, opaque
Capability Handles, block yield re-entry, three-class error attribution,
output capture, and a warm Sandbox pool.

| Feature | Entry Points | Notes |
|---------|-------------|-------|
| ✅ [F-01 Sandbox instantiation](docs/behavior/lifecycle.md) | [lib/kobako/sandbox.rb](lib/kobako/sandbox.rb) | B-22 (per-Thread isolation) is exercised only indirectly through the pool contention tests; no test cites it |
| ✅ [F-02 Service binding](docs/behavior/registration.md) | [lib/kobako/catalog/services.rb](lib/kobako/catalog/services.rb) | — |
| ✅ [F-04 Synchronous mruby source execution (`#eval`)](docs/behavior/lifecycle.md) | [lib/kobako/sandbox.rb](lib/kobako/sandbox.rb) | — |
| ✅ [F-05 Guest-initiated Transport dispatch](docs/behavior/dispatch.md) | [lib/kobako/transport/dispatcher.rb](lib/kobako/transport/dispatcher.rb) | — |
| ✅ [F-06 Capability Handle encoding and referencing](docs/behavior/dispatch.md) | [lib/kobako/catalog/handles.rb](lib/kobako/catalog/handles.rb) | — |
| ✅ [F-07 Three-class error attribution and raising](docs/behavior/errors.md) | [lib/kobako/outcome.rb](lib/kobako/outcome.rb) | E-26 (guest-entry envelope decode failure) has no exercising test — not reachable through the public API |
| ✅ [F-08 Guest output capture](docs/behavior/lifecycle.md) | [lib/kobako/capture.rb](lib/kobako/capture.rb) | — |
| ✅ [F-09 Host–guest message codec](docs/wire-codec.md) | [lib/kobako/codec/](lib/kobako/codec/) | — |
| ✅ [F-10 Reproducible build pipeline](SPEC.md#code-organization) | [tasks/wasm/build.rake](tasks/wasm/build.rake) | Verified by build-time gates (`rake anchors`, double-bake byte-identity, gemspec whitelist), not `test/` |
| ✅ [F-11 Multi-layer test and benchmark suite](SPEC.md#testing-style) | [test/](test/) | Benchmarks live in [benchmark/](benchmark/) with the gate in `tasks/bench/`; the anchor baseline advances only by deliberate re-bless |
| ✅ [F-12 Guest block reception and yield re-entry](docs/behavior/yield.md) | [lib/kobako/transport/yielder.rb](lib/kobako/transport/yielder.rb) | — |
| ✅ [F-13 Snippet preloading (`#preload`)](docs/behavior/invocation.md) | [lib/kobako/catalog/snippets.rb](lib/kobako/catalog/snippets.rb) | — |
| ✅ [F-14 Synchronous entrypoint dispatch (`#run`)](docs/behavior/invocation.md) | [lib/kobako/sandbox.rb](lib/kobako/sandbox.rb) | — |
| ✅ [F-15 Warm Sandbox pool checkout (`Kobako::Pool`)](docs/behavior/runtime.md) | [lib/kobako/pool.rb](lib/kobako/pool.rb) | — |
