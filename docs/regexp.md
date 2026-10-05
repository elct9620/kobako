# kobako-regexp — Regexp / MatchData

The intent and scope of the guest `Regexp` / `MatchData` capability. The
capability is opt-in, and [variants](variants.md) lists the Guest
Binaries that carry it. Its behaviors are scenarios in three feature
files.

| Feature file | Covers |
|---|---|
| [regexp](spec/behavior/regexp.md) | the pattern object, match globals, presence per Guest Binary |
| [regexp-string](spec/behavior/regexp-string.md) | the `String` methods that take a pattern |
| [regexp-matchdata](spec/behavior/regexp-matchdata.md) | what a successful match hands back |

## Intent

### Purpose

A kobako guest runs untrusted mruby with no ambient regexp engine.
kobako-regexp gives the guest `Regexp`, `MatchData`, and the `String`
integration methods as ordinary Ruby. Guest code then matches, captures,
substitutes, and splits entirely inside the sandbox.

| Reader | Works with |
|---|---|
| Guest mruby author | pattern-matching code |
| Host App author | the wire-projected results of that matching |

### Compute Boundary

Matching is a guest-internal compute capability, the pure-compute peer
of the IO / Kernel surface ([io](spec/behavior/io.md)). `Regexp` and
`MatchData` never cross the boundary; a returned value follows the
ordinary return-value semantics of [sandbox](spec/behavior/sandbox.md).

```
+---------------- guest ----------------+         +--- host ---+
| Regexp, MatchData  (stay inside)      |         |            |
| substring, index, captures, map, nil  | -wire-> | wire value |
+---------------------------------------+         +------------+
```

### MRI Fidelity

The surface is a curated subset of MRI's `Regexp` / `MatchData` API and
the `String` integration around it, implemented over fancy-regex.

| Aspect | Rule |
|---|---|
| Constructs | exactly the Surface below, nothing added |
| Behavior | MRI's, unless a scenario states otherwise |
| Engine | an implementation choice below this contract |
| Backtracking bound | MRI's answer or none, never a different one |

The bound is sized for genuinely unbounded matches. It is not raised to
cover patterns MRI still answers.

## Scope

### Surface

The guest sees exactly these constructs.

| Group | Members |
|-------|---------|
| `Regexp` construction | literal `/pattern/imx`; `Regexp.new(source[, options])`; `Regexp.compile` (alias) |
| `Regexp` class methods | `escape` / `quote`; `last_match`; `last_match=` |
| `Regexp` instance | `match`, `match?`, `=~`, `===`, `source`, `options`, `casefold?`, `named_captures`, `names`, `inspect`, `to_s`, `==`, `dup` / `clone` |
| `Regexp` constants | `IGNORECASE`, `EXTENDED`, `MULTILINE` |
| `MatchData` readers | `[]`, `begin`, `end`, `offset`, `captures`, `named_captures`, `names`, `size` / `length` |
| `MatchData` context | `pre_match`, `post_match`, `string`, `regexp`, `to_a`, `to_s`, `dup` / `clone` |
| `String` integration | `=~`, `match`, `match?`, `scan`, `gsub`, `sub`, `split`, `index`, `[]` / `slice`, `[]=`, `slice!` |
| `Kernel` | `=~` |
| Match globals | `$~`, `$1`..`$9`, `$&`, `` $` ``, `$'`, `$+` |
| Errors | `RegexpError` |

### Non-goals

The capability deliberately leaves out these parts of CRuby's surface.

| Excluded | In its place |
|----------|--------------|
| The full CRuby `Regexp` / `MatchData` API | only the curated Surface above |
| `Encoding` objects; character-based offsets | byte-based offsets and slices |
| Option constants beyond `IGNORECASE` / `EXTENDED` / `MULTILINE` | the MRI-aligned `Regexp#options` bits |
| `Regexp.version`, the `set_global_variables` toggle family | always-on match globals |
