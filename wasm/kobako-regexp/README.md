# kobako-regexp

Sandbox `Regexp` / `MatchData` capability gem for [kobako](https://github.com/elct9620/kobako)
mruby guests, built on [beni](https://crates.io/crates/beni).

Backs guest `Regexp` and `MatchData` with the pure-Rust
[`fancy-regex`](https://crates.io/crates/fancy-regex) engine, defined
entirely through the `beni` typed wrapper — no mrblib, no C mrbgem. A
Guest Binary shell composes it as an optional capability gem the same way
it composes `kobako-io`.

The Ruby-visible surface tracks the curated regexp engine's coverage, not
the full CRuby `Regexp` / `MatchData` API: there are no `Encoding`
objects, and match offsets and substring slices are byte-based. The
`unicode` cargo feature (default off) adds Unicode property classes
(`\p{...}`) and Unicode case folding at the cost of the engine's Unicode
tables.
