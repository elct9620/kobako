# kobako-regexp

Sandbox `Regexp` / `MatchData` capability gem for [kobako](https://github.com/elct9620/kobako)
mruby guests, built on [beni](https://crates.io/crates/beni).

Backs guest `Regexp` and `MatchData` with the pure-Rust
[`fancy-regex`](https://crates.io/crates/fancy-regex) engine, defined
entirely through the `beni` typed wrapper — no mrblib, no C mrbgem.

| Class | What the guest gets |
|---|---|
| `Regexp` | literals, `new` / `compile` / `escape` / `quote`, `match` / `match?` / `=~` / `===`, `source` / `options` / `casefold?` / `names` / `named_captures`, `inspect` / `to_s` / `==`, `last_match`, and the `$~` / `$1` match globals |
| `MatchData` | positional and named groups, byte offsets (`begin` / `end` / `offset`), `pre_match` / `post_match` / `string` / `regexp`, `to_a` / `to_s`, `dup` / `clone` |
| `String` | the pattern-taking methods — `=~`, `match`, `match?`, `sub` / `gsub`, `split`, `scan`, `index`, `[]` / `[]=`, `slice` / `slice!` |

Unlike the always-present `kobako-io`, a guest shell composes this gem
only when it needs `Regexp` / `MatchData`; it is meant to ship as its own
Guest Binary variant rather than as part of the default guest.

The Ruby-visible surface tracks the curated regexp engine's coverage, not
the full CRuby `Regexp` / `MatchData` API: there are no `Encoding`
objects, and match offsets and substring slices are byte-based.

## Limitations

| Area | What holds |
|---|---|
| The `unicode` feature | gates Unicode property classes (`\p{...}`) **and** case-insensitive matching — fancy-regex's flag is coarse, so with `unicode` off every `(?i)` pattern is rejected, and a guest using `/i` needs it on. ASCII `\d` / `\w` / `\s` are rewritten to explicit classes either way |
| Subject encoding | subjects match as UTF-8; a string that is not valid UTF-8 is treated as empty, never matching and never crashing. Byte-oriented matching is out of scope |
| Backtracking | a fancy pattern (backreferences, look-around) exceeding the engine's backtracking limit raises `RegexpError` rather than running unbounded; the host sandbox's wall-clock and memory caps remain the ultimate bound |

## Usage

```toml
[dependencies]
kobako-regexp = { version = "0.17.0", features = ["unicode"] } # x-release-please-version
beni = "0.18"
```

```rust
mrb.init_gem::<kobako_regexp::KobakoRegexp>()?;
```

In a kobako guest shell the call lives in the `MrbGuest::init_gems`
hook; the in-repo `kobako-wasm` shell composes it under its `regexp` /
`regexp-unicode` features into the `kobako+regexp` Guest Binary variants.

## License

Apache-2.0
