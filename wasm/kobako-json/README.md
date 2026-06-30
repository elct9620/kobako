# kobako-json

Sandbox `JSON` capability gem for [kobako](https://github.com/elct9620/kobako)
mruby guests, built on [beni](https://crates.io/crates/beni).

Backs a guest `JSON` module — `parse`, `generate`, `pretty_generate` — with
the pure-Rust [`serde_json`](https://crates.io/crates/serde_json) engine,
defined entirely through the `beni` typed wrapper — no mrblib, no C mrbgem.

Unlike the always-present `kobako-io`, a guest shell composes this gem only
when it needs `JSON`; it ships as its own Guest Binary variant rather than as
part of the default guest.

The Ruby-visible surface is a curated subset of MRI's `JSON` module — `parse`
(with `symbolize_names:`), `generate`, and `pretty_generate` — not the full
CRuby API. An object joins `generate` by overriding `Object#as_json` to return
a JSON-native value; the raising default refuses any object that has not opted
in, so a host capability reference is never serialized.

## Installation

```toml
kobako-json = "0.6.1" # x-release-please-version
```

## License

Apache-2.0
