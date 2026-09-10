# kobako-json — JSON parse / generate

The intent and scope of the guest `JSON` capability. Its behaviors are
scenarios in [`json.md`](spec/behavior/json.md).

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
peer of the IO / Kernel surface ([`io.md`](spec/behavior/io.md)) and the Regexp
surface ([`regexp.md`](regexp.md)). The `JSON` module is not among the 11 wire
types and never crosses the boundary; `parse` yields ordinary mruby values
(`nil` / bool / `Integer` / `Float` / `String` / `Array` / `Hash`) and
`generate` consumes them. A value the guest hands back to the host reduces to a
wire type by the ordinary return-value semantics of
[`sandbox.md`](spec/behavior/sandbox.md); JSON adds no wire type and no ext code.

Untrusted JSON is parsed by a memory-safe engine; the parser is an
implementation choice below this contract. The surface is a curated subset of
MRI's `JSON` module — exactly the constructs catalogued under Surface — and
follows MRI within that subset except where a scenario states otherwise.

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
| Guest generates a `Kobako::Handle` or un-opted object | `JSON.generate(handle)` | `JSON::GeneratorError`, no host dispatch ([`JS-042`](spec/behavior/json.md), [`JS-035`](spec/behavior/json.md)) |

### Non-goals

| Excluded | In its place |
|----------|--------------|
| The full CRuby `JSON` API (`dump` / `load`, `create_additions`, `JSON.stringify`) | only the curated Surface above |
| `NaN` / `Infinity` generation | `generate` raises `JSON::GeneratorError`, as CRuby does without `allow_nan:` |
| CRuby's `to_s`-degrade of an un-opted object | a fail-loud `JSON::GeneratorError` ([`JS-035`](spec/behavior/json.md)) |
| A raw `to_json` string-splice customization seam | the value-returning `as_json` hook, so the gem owns escaping and well-formedness |
| Serializing a host capability reference (`Kobako::Handle` / a bound constant) | refused outbound, unforgeable inbound ([`JS-042`](spec/behavior/json.md), [`JS-041`](spec/behavior/json.md)) |
| A byte-for-byte match of another `JSON` implementation's `pretty_generate` layout | the capability's own committed indented layout |
