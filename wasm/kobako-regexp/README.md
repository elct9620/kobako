# kobako-regexp

Sandbox `Regexp` / `MatchData` capability gem for [kobako](https://github.com/elct9620/kobako)
mruby guests, built on [beni](https://crates.io/crates/beni).

It backs guest `Regexp` and `MatchData` with a pure-Rust engine.

| Part | What it is |
|---|---|
| Engine | [`fancy-regex`](https://crates.io/crates/fancy-regex) |
| Definition | entirely through the `beni` typed wrapper |
| Left out | mrblib, C mrbgem |

## Guest Surface

The surface tracks the curated regexp engine's coverage, not the full
CRuby `Regexp` / `MatchData` API. There are no `Encoding` objects, and
match offsets and substring slices are byte-based.

| Class | Area | What the guest gets |
|---|---|---|
| `Regexp` | construction | literals, `new`, `compile`, `escape`, `quote` |
| `Regexp` | matching | `match`, `match?`, `=~`, `===`, `last_match` |
| `Regexp` | reading | `source`, `options`, `casefold?`, `names`, `named_captures` |
| `Regexp` | display | `inspect`, `to_s`, `==` |
| `Regexp` | match globals | `$~`, `$1` |
| `MatchData` | groups | positional and named groups, `to_a`, `to_s` |
| `MatchData` | byte offsets | `begin`, `end`, `offset` |
| `MatchData` | context | `pre_match`, `post_match`, `string`, `regexp` |
| `MatchData` | copies | `dup`, `clone` |
| `String` | pattern-taking methods | `=~`, `match`, `match?`, `sub`, `gsub`, `split`, `scan` |
| `String` | pattern-taking methods | `index`, `[]`, `[]=`, `slice`, `slice!` |
| `Symbol`, `nil` | match operator | `=~` |

## Limitations

Each row names what holds for a guest's patterns or subjects.

| Area | What holds |
|---|---|
| Unicode property classes (`\p{...}`) | need the `unicode` feature |
| Case-insensitive matching | needs the `unicode` feature |
| ASCII `\d` / `\w` / `\s` | rewritten to explicit classes either way |
| Subject encoding | UTF-8; an invalid subject is treated as empty |
| Backtracking | past the engine's limit raises `RegexpError` |

fancy-regex's flag is coarse, so with `unicode` off every `(?i)` pattern
is rejected, and a guest using `/i` needs it on. A subject that is not
valid UTF-8 never matches and never crashes. Byte-oriented matching is
out of scope.

A fancy pattern, one with backreferences or look-around, stops at the
backtracking limit rather than running unbounded. The host sandbox's
wall-clock and memory caps remain the ultimate bound.

## Usage

```toml
[dependencies]
kobako-regexp = { version = "0.17.0", features = ["unicode"] } # x-release-please-version
beni = "0.22"
```

```rust
mrb.init_gem::<kobako_regexp::KobakoRegexp>()?;
```

A guest shell composes this gem only when it needs `Regexp` /
`MatchData`, unlike the always-present `kobako-io`. It ships as its own
Guest Binary variant rather than as part of the default guest.

In a kobako guest shell the call lives in the `MrbGuest::init_gems` hook.
The in-repo `kobako-wasm` shell composes it under its `regexp` /
`regexp-unicode` features into the `kobako+regexp` Guest Binary variants.

## License

Licensed under [Apache-2.0](https://github.com/elct9620/kobako/blob/main/LICENSE).
