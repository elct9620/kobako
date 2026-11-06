# Guest Binary Variants

The Guest Binary ships in named variants that compose optional
capability gems onto a common base. Variants are the artifacts we ship;
the interfaces a third party replaces are in
[customization](customization.md).

```
base -+- alone ------------ kobako.wasm
      +- + regexp --------- kobako+regexp.wasm
      +- + regexp-unicode - kobako+regexp-unicode.wasm
      +- + json ----------- kobako+json.wasm
      +- + regexp + json -- kobako+full.wasm
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
| File ([file](../spec/behavior/file.md)) | the guest's half of a Mount, reaching nothing until one is mounted |

## Variant Matrix

Each variant names the capabilities it adds beyond the base.

| Variant | Artifact | Capabilities beyond the base |
|---------|----------|------------------------------|
| default | `kobako.wasm` | none — pure compute |
| regexp | `kobako+regexp.wasm` | ASCII Regexp / MatchData |
| regexp-unicode | `kobako+regexp-unicode.wasm` | Regexp / MatchData with Unicode case-insensitive matching |
| json | `kobako+json.wasm` | JSON parse / generate |
| full | `kobako+full.wasm` | ASCII Regexp + JSON |

Choose `regexp-unicode` when guest code needs case-insensitive patterns, word
boundaries, `\p{...}` properties, POSIX brackets beyond ASCII, or `(?u)`.
[regexp](../spec/behavior/regexp.md) states what each ASCII variant
refuses.

## Guest Surface

Each capability is a chosen subset of CRuby, followed as closely as the guest
can. A difference the guest cannot avoid stays, and the capability's spec
states it; the sections below show where each subset ends.

| Surface | Present in |
|---|---|
| Base Gems | every variant |
| Regexp | `regexp`, `regexp-unicode`, `full` |
| JSON | `json`, `full` |

### Base Gems

The base holds mruby core and an allowlist of core gems. No I/O, network,
sleep, or random-seed gem enters it.

| Area | Gems |
|---|---|
| compiling and errors | `mruby-compiler`, `mruby-error`, `mruby-metaprog` |
| numbers | `mruby-bigint`, `mruby-numeric-ext`, `mruby-sprintf` |
| collections | `mruby-array-ext`, `mruby-hash-ext`, `mruby-enum-ext`, `mruby-range-ext` |
| objects and text | `mruby-object-ext`, `mruby-proc-ext`, `mruby-string-ext`, `mruby-symbol-ext` |

Guest IO writes to stdout, stderr, and the files it opened, and to no other
descriptor.

### Regexp Surface

The Regexp capability gives `Regexp`, `MatchData`, and the pattern methods on
`String`. A pattern never crosses to the host; only its result does.

| Has | Lacks |
|---|---|
| `/pattern/imx`, `Regexp.new`, `escape`, `last_match` | `Encoding` objects, `Regexp.version` |
| `match`, `match?`, `=~`, `===`, `named_captures` | switches turning the match globals off |
| `$~`, `$1`..`$9`, `` $` ``, `$'`, `$+` | case-insensitive patterns on the ASCII builds |

Offsets count bytes. On `regexp` and `full`, a case-insensitive pattern raises
`RegexpError` rather than match narrower.

### JSON Surface

The JSON capability gives a `JSON` module for reading and building bodies. A
value crosses to the host as any other value does.

| Has | Lacks |
|---|---|
| `parse(str, symbolize_names:)` | `dump`, `load`, `unsafe_load`, `load_file` |
| `generate`, `pretty_generate` | `parse!`, `JSON[]` |
| `Object#as_json` | a `to_json` string splice |

It refuses a value with no JSON form, such as a non-finite number, bytes that
are not text, or a non-scalar key. Parsing never yields a Handle, and
generating refuses one wherever it sits.

## Artifact Naming

The suffix encodes capability composition, not a version.

| Name | Names |
|---|---|
| `kobako.wasm` | the default, fixed name |
| `kobako-<version>.wasm` | the default as a Release asset |
| `kobako+<cap>.wasm` | a capability variant |
| `kobako+<cap>-<version>.wasm` | that variant as a Release asset |

`<cap>` names an opt-in capability axis (`regexp`, `regexp-unicode`,
`json`) or a composition shorthand (`full` = ASCII regexp + JSON).

## Packaging Policy

The published gem bundles exactly one Guest Binary, `data/kobako.wasm`.
This keeps the install
footprint minimal; a Host App that needs a capability downloads the
matching variant.

| Guest Binary | Ships as |
|---|---|
| `data/kobako.wasm` | bundled in the gem, and a Release asset |
| capability variants | Release assets, or a local build |

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
byte-identical artifact; a build that cannot reproduce its bake fails.

| Task | Produces |
|---|---|
| `rake wasm:build` | `kobako.wasm` |
| `rake wasm:build:regexp` | `kobako+regexp.wasm` |
| `rake wasm:build:regexp_unicode` | `kobako+regexp-unicode.wasm` |
| `rake wasm:build:json` | `kobako+json.wasm` |
| `rake wasm:build:full` | `kobako+full.wasm` |

## Rust SDK

The Rust SDK bundles no Guest Binary, so every Sandbox names the variant it
loads.

```rust
let sandbox = Sandbox::new("path/to/kobako+json.wasm", Options::default())?;
```
