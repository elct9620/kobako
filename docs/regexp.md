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

The surface is a curated subset of MRI's `Regexp` / `MatchData` API and the
`String` integration around it — exactly the constructs catalogued under
Surface below — implemented over fancy-regex. Nothing outside that set is
added even when MRI has it; within the set the behavior follows MRI except
where a behavior below states otherwise. There are
no `Encoding` objects; match offsets and substring slices are byte-based, and
the matching engine is an implementation choice below this contract.

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
| Engine-intrinsic surface — option constants beyond `IGNORECASE` / `EXTENDED` / `MULTILINE`, `Regexp.version`, and the `set_global_variables` toggle family | the MRI-aligned `Regexp#options` bits and always-on match globals |

## Behavior

Each behavior carries an `RX-xx` anchor. Match offsets, substring slices, and
position arguments are byte-based throughout.

### RX-01 — Pattern compilation and options

A regexp literal `/pattern/imx`, `Regexp.new(source[, options])`, and
`Regexp.compile` build a compiled pattern. `options` is an Integer mask
(`IGNORECASE | EXTENDED | MULTILINE`), a letter String (`"imx"`), or omitted.
A pattern that fails to compile raises `RegexpError`.

The shorthand classes `\d` / `\w` / `\s` match their ASCII sets only, as in
MRI, and the negations `\D` / `\W` / `\S` complement those ASCII sets —
except the negated forms inside a character class (`[\D]`, `[\W]`, `[\S]`),
which match by Unicode category: a non-ASCII digit such as `５` matches
`\D` but not `[\D]`.

| Member | Result |
|--------|--------|
| `#source` | the pattern String |
| `#options` | the MRI option bits as an Integer (`IGNORECASE` 1, `EXTENDED` 2, `MULTILINE` 4) |
| `#casefold?` | whether `IGNORECASE` is set |
| `#inspect` | `/source/flags`, the source rendered as a regexp literal: `/` escapes to `\/`, and a non-printable character escapes to `\xHH` (uppercase hex); printable characters — including multibyte UTF-8 — and the whitespace controls (tab, newline, vertical tab, form feed, carriage return) pass through literally |
| `#to_s` | `(?enabled-disabled:source)` with the flag letters in `m`, `i`, `x` order; the `-disabled` block is omitted when `m`, `i`, and `x` are all enabled. When the whole source is a single inline-flag group `(?flags:inner)` (including the flag-less `(?:inner)`), its enabled and disabled flags combine with the outer options and `inner` becomes the body; a source that is not one whole-span group is wrapped verbatim |
| `#named_captures` | a Hash mapping each capture name to its group numbers |
| `#names` | the capture names in declaration order |
| `#==` | true when another `Regexp` has equal source and options |
| `#dup` / `#clone` | an independent copy with the same pattern and options |
| `Regexp.escape` / `Regexp.quote` | the argument with the regexp metacharacters and whitespace MRI's `Regexp.escape` escapes backslash-escaped |

### RX-02 — Matching and match globals

`#match` returns a `MatchData` on a hit and `nil` on a miss; given a block, it
yields the `MatchData` on a hit and returns the block's result, and does not
call the block on a miss. `#match?` returns a boolean, `#=~` the match's start
index or `nil`, and `#===` a boolean. The subject must be a `String` or
`Symbol`; `nil` is no match (`#match` / `#=~` return `nil`, `#match?` / `#===`
return `false`), and any other operand raises `TypeError` — except `#===`,
which rescues it to `false`. An optional position argument starts the
search at that byte offset; a position outside the subject yields no match,
a negative position counts back from the end, and a position inside a
multibyte character snaps down to its char boundary.

A successful match refreshes the match globals; a miss clears them.

| Global | Value after a hit |
|--------|-------------------|
| `$~` | the `MatchData` |
| `$1`..`$9` | the numbered captures |
| `$&` | the whole match |
| `` $` `` / `$'` | the text before / after the match |
| `$+` | the last capture group that participated, `nil` when the pattern has no groups |

`Regexp.last_match` reads the most recent match (`$~`); `Regexp.last_match=`
overwrites it, letting a caller save and restore the match around an inner one.

### RX-03 — MatchData accessors

A `MatchData` is an immutable snapshot of one match; `MatchData.new` raises
`NoMethodError`, as a `MatchData` only arises from matching.

| Member | Result |
|--------|--------|
| `#[]` | a group by Integer index or capture name (Symbol/String); a start+length or a Range returns that slice of the group list; a negative index counts from the end; an undefined name raises `IndexError` |
| `#begin` / `#end` / `#offset` | byte offsets for a group; a non-participating group is `nil`; an index past the group count or an undefined name raises `IndexError` |
| `#captures` | the groups, excluding the whole match |
| `#named_captures` | each capture name mapped to its captured String (`symbolize_names: true` keys by Symbol) |
| `#names` | the capture names |
| `#pre_match` / `#post_match` | the text before / after the match |
| `#to_a` | the whole match followed by each capture |
| `#to_s` | the whole match |
| `#size` / `#length` | the whole match plus the group count |
| `#string` | the matched subject |
| `#regexp` | the originating `Regexp` |
| `#dup` / `#clone` | a copy carrying the same snapshot |

### RX-04 — Substitution

`String#gsub` and `String#sub` replace matches with a replacement String, a
Hash keyed by the whole match, or a block's result. A String replacement
expands `\0`..`\9` and `\k<name>` backreferences (an undefined name raises
`IndexError`, a malformed `\k` raises `RegexpError`); a `\\` is a literal
backslash and any other `\x` stays its two literal characters. A replacement
argument takes precedence over a block. An exception raised in a block
propagates.

`gsub` with neither a block nor a replacement returns an Enumerator over the
matches (`to_enum`), which requires the guest to compose Enumerator support;
`sub` with neither raises `ArgumentError`.

### RX-05 — Scan and split

`String#scan` collects each non-overlapping match — the whole match for a
group-less pattern, otherwise an Array of the groups (a non-participating group
is `nil`). Given a block it yields each and returns the receiver, propagating
any exception the block raises.

`String#split` divides the subject on the pattern, interleaving each match's
capture groups between the fields (a non-participating group is omitted, unlike
`scan`). A positive limit caps the
field count, leaving the remainder as the last field; an omitted or `0` limit
drops trailing empty fields; a negative limit keeps them. A non-`Regexp`
argument delegates to the core method.

### RX-06 — String matching, position, and slicing

| Member | Behavior |
|--------|----------|
| `#=~` | matches a `Regexp` operand and returns the index or `nil`; a `String` operand raises `TypeError`; any other receiver falls through to `Kernel#=~` (`nil`) |
| `#match` / `#match?` | forward `self` to the pattern's `#match` / `#match?`; the pattern is a `Regexp` and a non-`Regexp` raises `TypeError` (a String is not coerced into a pattern); `#match` forwards a block |
| `#index(pattern[, pos])` | the byte offset of the first match at or after `pos` (handled as the RX-02 position argument), or `nil` |
| `#[]` / `#slice` | with a `Regexp` (and optional group) returns the matched substring or that capture |
| `#[]=` | overwrites the matched region — the whole match, or capture group `n` — and raises `IndexError` on no match |
| `#slice!` | removes and returns the matched (or indexed) portion, leaving `$~` unchanged for the `Regexp` form |

A non-`Regexp` argument to `#index` / `#[]` / `#[]=` / `#slice!` delegates to
the core String method.
