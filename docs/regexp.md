# kobako-regexp — Regexp / MatchData

The behavior contract for the guest `Regexp` / `MatchData` capability,
expanding the [B-41](../SPEC.md) capability anchor. Behaviors carry `RX-xx`
anchors that are append-only and live only in this file; each maps to a test
in `test/regexp/`.

## Intent

### Purpose

A kobako guest runs untrusted mruby with no ambient regexp engine.
kobako-regexp gives the guest `Regexp` and `MatchData` as ordinary Ruby
classes plus the `String` integration methods, so guest code matches,
captures, substitutes, and splits entirely inside the sandbox.

### Users

Guest mruby authors writing pattern-matching code, and Host App authors who
receive the wire-projected results of that matching.

### Impacts

Matching is a guest-internal compute capability — the pure-compute peer of the
IO / Kernel surface (B-04). `Regexp` and `MatchData` are not among the 12 wire
types and never cross the boundary; a value the guest hands back to the host
reduces to a wire type first (a captured substring as `str`, a match index as
`int`, a capture list as `array`, a `named_captures` map as `map`, `nil` for
no match, or a `Symbol`). A bare `Regexp` or `MatchData` in a returned
position is a non-wire value governed by the ordinary return-value semantics
(B-06).

Coverage is a curated subset of the CRuby `Regexp` / `MatchData` API, not its
full surface. There are no `Encoding` objects; match offsets and substring
slices are byte-based. The matching engine is an implementation choice below
this contract.

Where the curated subset and the original C `mruby-onig-regexp` engine
disagree, behavior follows MRI.

## Scope

### Surface

The guest sees exactly these constructs.

| Group | Members |
|-------|---------|
| `Regexp` construction | literal `/pattern/imx`; `Regexp.new(source[, options])`; `Regexp.compile` (alias) |
| `Regexp` class methods | `escape` / `quote`; `last_match`; `last_match=` |
| `Regexp` instance | `match`, `match?`, `=~`, `===`, `source`, `options`, `casefold?`, `named_captures`, `names`, `inspect`, `to_s`, `==`, `dup` / `clone` |
| `Regexp` constants | `IGNORECASE` (1), `EXTENDED` (2), `MULTILINE` (4) |
| `MatchData` | `[]`, `begin`, `end`, `offset`, `captures`, `named_captures`, `names`, `size` / `length`, `pre_match`, `post_match`, `string`, `regexp`, `to_a`, `to_s`, `dup` / `clone`; `new` is not constructible |
| `String` integration | `=~`, `match`, `match?`, `scan`, `gsub`, `sub`, `split`, `index`, `[]` / `slice`, `[]=`, `slice!` |
| `Kernel` | `=~` (returns `nil`) |
| Match globals | `$~`, `$1`..`$9`, `$&`, `` $` ``, `$'`, `$+` |
| Errors | `RegexpError` (a `StandardError` subclass the gem defines) |

### Journeys

| Context | Action | Outcome |
|---------|--------|---------|
| Guest holds a `String` | matches a pattern (`=~` / `match` / `match?` / literal) | a `MatchData`, an Integer index, `nil`, and the refreshed match globals |
| Guest holds a `MatchData` | reads a group by index or name, an offset, or `pre_match` / `post_match` | a captured substring or byte offset |
| Guest holds a `String` | `gsub` / `sub` with a replacement or block | a new `String` with matches substituted |
| Guest holds a `String` | `scan` / `split` on a pattern | an `Array` of matches or fields |
| Guest returns a captured substring | the value crosses the boundary | the host receives a wire `str` (B-41) |

### Non-goals

| Excluded | In its place |
|----------|--------------|
| The full CRuby `Regexp` / `MatchData` API | only the curated Surface above |
| `Encoding` objects; character-based offsets | byte-based offsets and slices |
| Onigmo engine-intrinsic surface — option constants beyond `IGNORECASE` / `EXTENDED` / `MULTILINE`, `Regexp.version`, and the `set_global_variables` toggle family | the MRI-aligned `Regexp#options` bits and always-on match globals |
