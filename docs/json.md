# kobako-json

The intent and scope of the guest `JSON` capability. The capability is
opt-in, and [variants](variants.md) lists the Guest Binaries that carry
it.

| Feature file | Covers |
|---|---|
| [json](spec/behavior/json.md) | parsing, generating, refusals, presence per Guest Binary |

## Intent

### Purpose

A kobako guest runs untrusted mruby with no ambient JSON engine.
kobako-json gives the guest a `JSON` module, so guest code reads
response bodies and builds request bodies entirely inside the sandbox.
That is the natural shape for scripts that make API calls.

| Reader | Works with |
|---|---|
| Guest mruby author | parsing untrusted input and generating output |
| Host App author | the wire-projected mruby values that result |

### Compute Boundary

Parsing and generation are a guest-internal compute capability, the
pure-compute peer of the IO / Kernel surface ([io](spec/behavior/io.md))
and the [Regexp](regexp.md) surface. JSON adds no wire type and no ext
code; a returned value follows the ordinary return-value semantics of
[sandbox](spec/behavior/sandbox.md).

```
+---------------- guest ----------------+         +--- host ---+
| JSON.parse / JSON.generate (inside)   |         |            |
| native mruby values                   | -wire-> | wire value |
+---------------------------------------+         +------------+
```

### MRI Fidelity

The surface is a curated subset of MRI's `JSON` module.

| Aspect | Rule |
|---|---|
| Constructs | exactly the Surface below, nothing added |
| Behavior | MRI's, unless a scenario states otherwise |
| Parser | a memory-safe engine, an implementation choice below this contract |

## Scope

### Surface

The guest sees exactly these constructs.

| Group | Members |
|-------|---------|
| `JSON` module | `parse(str, **opts)`, `generate(obj)`, `pretty_generate(obj)` |
| `parse` options | `symbolize_names:` |
| Serialization hook | `Object#as_json`, the opt-in for `generate` |
| Errors | `JSON::JSONError`, `JSON::ParserError`, `JSON::GeneratorError` |

### Non-goals

The capability deliberately leaves out these parts of CRuby's surface.

| Excluded | In its place |
|----------|--------------|
| The full CRuby `JSON` API (`dump` / `load`, `create_additions`, `JSON.stringify`) | only the curated Surface above |
| `NaN` / `Infinity` generation | a `JSON::GeneratorError`, as CRuby without `allow_nan:` |
| CRuby's `to_s`-degrade of an un-opted object | a fail-loud `JSON::GeneratorError` |
| A raw `to_json` string-splice customization seam | the value-returning `as_json` hook, so the gem owns escaping |
| Serializing a host capability reference (`Kobako::Handle` / a bound constant) | refused outbound, unforgeable inbound |
| A byte-for-byte match of another `pretty_generate` layout | the capability's own committed indented layout |
