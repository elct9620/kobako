# kobako-json — JSON parse / generate

The behavior contract for the guest `JSON` capability, expanding the
[B-52](../SPEC.md) capability anchor. Behaviors carry `JS-xx` anchors that are
append-only and live only in this file; each maps to a test in `test/json/`.

## Intent

### Purpose

A kobako guest runs untrusted mruby with no ambient JSON engine. kobako-json
gives the guest a `JSON` module — `parse`, `generate`, `pretty_generate` — so
guest code reads response bodies and builds request bodies entirely inside the
sandbox, the natural shape for scripts that make API calls.

### Users

Guest mruby authors parsing untrusted input and generating output, and Host App
authors who receive the wire-projected mruby values that result.

### Impacts

Parsing and generation are a guest-internal compute capability — the pure-compute
peer of the IO / Kernel surface (B-04) and the Regexp surface (B-41). The `JSON`
module is not among the 12 wire types and never crosses the boundary; `parse`
yields ordinary mruby values (`nil` / bool / `Integer` / `Float` / `String` /
`Array` / `Hash`) and `generate` consumes them. A value the guest hands back to
the host reduces to a wire type by the ordinary return-value semantics (B-06);
JSON adds no wire type and no ext code.

Untrusted JSON is parsed by a memory-safe engine; the parser is an
implementation choice below this contract. The surface is a curated subset of
MRI's `JSON` module — exactly the constructs catalogued under Surface — and
follows MRI within that subset except where a behavior below states otherwise.

### Availability

The capability is opt-in: the default Guest Binary ships without it. The
variants that carry it, the build tasks, and the packaging policy live in
[`docs/variants.md`](variants.md).

## Scope

### Surface

The guest sees exactly these constructs.

| Group | Members |
|-------|---------|
| `JSON` module | `parse(str, **opts)`, `generate(obj)`, `pretty_generate(obj)` |
| `parse` options | `symbolize_names:` (default `false`) |
| Serialization hook | `Object#as_json` — raising by default; an object opts into `generate` by overriding it to return a JSON-native value |
| Errors | `JSON::JSONError` (a `StandardError` subclass the gem defines); `JSON::ParserError` and `JSON::GeneratorError`, both subclasses of `JSON::JSONError` |

### Journeys

| Context | Action | Outcome |
|---------|--------|---------|
| Guest holds an untrusted JSON `String` | `JSON.parse(body)` | a tree of native mruby values, object member order preserved |
| Guest wants symbol keys | `JSON.parse(body, symbolize_names: true)` | the same tree with `Symbol` keys |
| Guest holds native mruby values | `JSON.generate(obj)` | a well-formed JSON `String` |
| Guest defines `as_json` on its own class | `JSON.generate(obj)` | the JSON for the value `as_json` returns |
| Guest generates a `Kobako::Handle` or un-opted object | `JSON.generate(handle)` | `JSON::GeneratorError`, no host dispatch (B-53) |

### Non-goals

| Excluded | In its place |
|----------|--------------|
| The full CRuby `JSON` API (`dump` / `load`, `create_additions`, `JSON.stringify`) | only the curated Surface above |
| `NaN` / `Infinity` generation | `generate` raises `JSON::GeneratorError`, as CRuby does without `allow_nan:` |
| CRuby's `to_s`-degrade of an un-opted object | a fail-loud `JSON::GeneratorError` (B-53) |
| A raw `to_json` string-splice customization seam | the value-returning `as_json` hook, so the gem owns escaping and well-formedness |
| Serializing a host capability reference (`Kobako::Handle` / `Member`) | refused outbound, unforgeable inbound (B-53) |

## Behavior

Each behavior carries a `JS-xx` anchor.

### JS-01 — Parse maps JSON values to JSON-native mruby types

`JSON.parse` maps each JSON value to its mruby counterpart: `null` → `nil`,
`true` / `false` → the booleans, a JSON string → `String`, a JSON number →
`Integer` or `Float` (JS-03), a JSON array → `Array`, and a JSON object → `Hash`
keyed by `String` (JS-02 for symbol keys). Parsing produces only these
JSON-native types — never a `Kobako::Handle` or any host capability (B-53).

### JS-02 — symbolize_names yields Symbol keys

`JSON.parse(str, symbolize_names: true)` makes every object key a `Symbol`
instead of a `String`; the default (`false`) keeps `String` keys. Values are
unaffected.

### JS-03 — Integer range policy

A JSON integer that fits the guest `Integer` width maps to `Integer`. A larger
magnitude that a `Float` still represents exactly — within the f64 53-bit
mantissa — maps to `Float`. A magnitude beyond exact `Float` range raises
`JSON::ParserError`: parsing never silently degrades an integer it cannot carry
without precision loss. A JSON real maps to `Float`.

### JS-04 — Parse preserves object member order

`JSON.parse` preserves the order in which members appear in a JSON object, so
the resulting `Hash` iterates in source order, matching CRuby.

### JS-05 — Malformed JSON raises JSON::ParserError

Input that is not well-formed JSON — a syntax error, a truncated document,
trailing content, or nesting beyond the depth bound (JS-09) — raises
`JSON::ParserError` inside the guest. Uncaught, it is attributed as
`Kobako::SandboxError` per E-04.

### JS-06 — Generate emits well-formed JSON

`JSON.generate` emits a compact, well-formed JSON `String` for JSON-native
values, with correct string escaping: `String` → JSON string, `nil` → `null`,
the booleans, `Integer` and `Float` numbers, `Array`, and `Hash`. A `Symbol`
value renders as its name. A `Hash` key renders as its string form when it is a
`String`, a `Symbol`, or a JSON-native scalar (a number, `nil`, or a boolean),
as in CRuby. Any other key raises `JSON::GeneratorError`: a JSON-native `Array`
or `Hash` is not a usable JSON key, and a `Kobako::Handle`, a `Member`, or any
other non-native object is refused through the same boundary as a non-native
value (B-53), never stringified through a host-dispatching `to_s`. The `as_json`
opt-in (JS-08) applies to values, never to keys. A `Float` that is
`NaN` or infinite raises `JSON::GeneratorError`. Output is always well-formed:
the gem owns escaping and never splices caller-provided text.

### JS-07 — pretty_generate emits indented JSON

`JSON.pretty_generate` emits the same value as `generate` in an indented
layout: two-space indentation per nesting level, a space after each `:`, one
object member or array element per line, and an empty `[]` / `{}` left inline.
The layout is the capability's own committed shape, not a byte-for-byte match of
any other `JSON` implementation; parsing its output yields the same tree as
parsing `generate`'s output.

### JS-08 — Generate serializes an opt-in object through as_json

A value the generator does not encode directly — anything but the JSON-native
types and `Symbol` (JS-06) — serializes only if its class overrides the raising
`Object#as_json` default.
`generate` calls `as_json` and encodes the value it returns, recursively, under
the same rules (escaping, depth bound, capability refusal). `Object#as_json` is
defined with a raising default, so an object that has not opted in raises
`JSON::GeneratorError` (B-53). The hook returns a value the gem encodes — to
serialize an object as `true`, its `as_json` returns the boolean `true`, not the
string `"true"`. `generate` consults `as_json` only; overriding `to_json` does
not change `generate`'s output.

### JS-09 — Nesting depth bound

`parse` and `generate` enforce a fixed maximum nesting depth of 128, owned by
the JSON capability. Parsing input nested deeper raises `JSON::ParserError`;
generating a structure nested deeper raises `JSON::GeneratorError`. The bound
caps recursion on untrusted input rather than exhausting the guest stack.
