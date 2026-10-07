# Guest Binary Variants

The Guest Binary ships in named variants that compose optional
capability gems onto a common base. Variants are the artifacts we ship;
the interfaces a third party replaces are in
[customization](customization.md).

```
base ─┬─ alone ──────────── kobako.wasm
      ├─ + regexp ───────── kobako+regexp.wasm
      ├─ + regexp-unicode ─ kobako+regexp-unicode.wasm
      ├─ + json ─────────── kobako+json.wasm
      └─ + regexp + json ── kobako+full.wasm
```

## Base Surface

Every variant, including the default, links the same base. The opt-in
axes are the Regexp capability ([regexp](../spec/behavior/regexp.md)) and
the JSON capability ([json](../spec/behavior/json.md)).

| Part | Role |
|---|---|
| mruby core | the interpreter |
| curated mrbgem allowlist | the standard-library subset |
| IO / Kernel write ([io](../spec/behavior/io.md)) | the base capability, never an opt-in axis |

## Variant Matrix

Each variant names the capabilities it adds beyond the base.

| Variant | Artifact | Capabilities beyond the base |
|---------|----------|------------------------------|
| default | `kobako.wasm` | none — pure compute |
| regexp | `kobako+regexp.wasm` | ASCII Regexp / MatchData |
| regexp-unicode | `kobako+regexp-unicode.wasm` | Regexp / MatchData with Unicode case-insensitive matching |
| json | `kobako+json.wasm` | JSON parse / generate |
| full | `kobako+full.wasm` | ASCII Regexp + JSON |

Choose `regexp-unicode` when guest code needs case-insensitive patterns.
[regexp](../spec/behavior/regexp.md) states what each ASCII variant
refuses.

## Artifact Naming

The suffix encodes capability composition, not a version.

```
kobako.wasm                    the default, fixed name
kobako+<cap>.wasm              a capability variant
kobako+<cap>-<version>.wasm    that variant as a Release asset
```

`<cap>` names an opt-in capability axis (`regexp`, `regexp-unicode`,
`json`) or a composition shorthand (`full` = ASCII regexp + JSON).

## Packaging Policy

The published gem bundles exactly one Guest Binary, by the gemspec's
file allowlist. This keeps the install footprint minimal; a Host App
that needs a capability downloads the matching variant.

| Guest Binary | Ships as |
|---|---|
| `data/kobako.wasm` | bundled in the gem |
| capability variants | GitHub Release assets, or a local build |

## Variant Selection

A Host App selects a variant per Sandbox by pointing `wasm_path:` at the
chosen binary. The binary fixes the capability surface a guest sees;
there is no runtime capability negotiation.

```ruby
Kobako::Sandbox.new(wasm_path: "path/to/kobako+json.wasm")
```

## Build Tasks

Every variant passes through the canonical boot bake
([mruby](../spec/behavior/mruby.md)). Re-baking the same inputs yields a
byte-identical artifact, gated by the reproducible-build pipeline.

| Task | Produces |
|---|---|
| `rake wasm:build` | `kobako.wasm` |
| `rake wasm:build:regexp` | `kobako+regexp.wasm` |
| `rake wasm:build:regexp_unicode` | `kobako+regexp-unicode.wasm` |
| `rake wasm:build:json` | `kobako+json.wasm` |
| `rake wasm:build:full` | `kobako+full.wasm` |
