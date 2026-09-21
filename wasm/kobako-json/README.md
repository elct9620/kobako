# kobako-json

Sandbox `JSON` capability gem for [kobako](https://github.com/elct9620/kobako)
mruby guests, built on [beni](https://crates.io/crates/beni).

Backs a guest `JSON` module with the pure-Rust
[`serde_json`](https://crates.io/crates/serde_json) engine, defined entirely
through the `beni` typed wrapper — no mrblib, no C mrbgem.

| Method | What it takes |
|---|---|
| `JSON.parse` | a JSON String, with `symbolize_names:` |
| `JSON.generate` | a value whose objects have opted in |
| `JSON.pretty_generate` | the same, indented |

Unlike the always-present `kobako-io`, a guest shell composes this gem only
when it needs `JSON`; it ships as its own Guest Binary variant rather than as
part of the default guest.

## Opting an object in

The Ruby-visible surface is a curated subset of MRI's `JSON` module, not the
full CRuby API. An object joins `generate` by overriding `Object#as_json` to
return a JSON-native value. The raising default refuses any object that has
not opted in, so a host capability reference is never serialized.

```ruby
class Point
  def as_json = { "x" => @x, "y" => @y }
end
```

## Installation

```toml
kobako-json = "0.17.0" # x-release-please-version
```

## License

Apache-2.0
