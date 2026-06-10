# kobako-regexp

Sandbox `Regexp` / `MatchData` capability gem for [kobako](https://github.com/elct9620/kobako)
mruby guests, built on [beni](https://crates.io/crates/beni).

Backs guest `Regexp` and `MatchData` with the pure-Rust
[`fancy-regex`](https://crates.io/crates/fancy-regex) engine, defined
entirely through the `beni` typed wrapper — no mrblib, no C mrbgem.

Unlike the always-present `kobako-io`, a guest shell composes this gem
only when it needs `Regexp` / `MatchData`; it is meant to ship as its own
Guest Binary variant rather than as part of the default guest.

The Ruby-visible surface tracks the curated regexp engine's coverage, not
the full CRuby `Regexp` / `MatchData` API: there are no `Encoding`
objects, and match offsets and substring slices are byte-based.

## Limitations

- The `unicode` cargo feature gates Unicode property classes (`\p{...}`)
  **and** case-insensitive matching. fancy-regex's flag is coarse, so with
  `unicode` off every `(?i)` pattern is rejected — a guest using `/i` needs
  it on. ASCII `\d` / `\w` / `\s` are rewritten to explicit classes either
  way.
- Subjects are matched as UTF-8. A string that is not valid UTF-8 is treated
  as empty (it never matches and never crashes); byte-oriented matching is
  out of scope.
- A fancy pattern (backreferences, look-around) that exceeds the engine's
  backtracking limit raises `RegexpError` rather than running unbounded; the
  host sandbox's wall-clock and memory caps remain the ultimate bound.
