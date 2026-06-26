# Guest Binary Variants — capability composition and packaging

The Guest Binary ships in named variants that compose optional capability gems
onto a common base. The governing summary lives in [`SPEC.md`](../SPEC.md)
§ Scope; this file is the per-variant reference for the matrix, the naming
convention, and the packaging policy.

## The base surface

Every variant — including the default — links the mruby core, the curated
mrbgem allowlist, and the IO / Kernel write capability (B-04). IO / Kernel is
not an opt-in axis; it is the base of every Guest Binary. The opt-in axes are
the Regexp capability (B-41) and the JSON capability (B-52).

## Variant matrix

| Variant | Artifact | Capabilities beyond the base |
|---------|----------|------------------------------|
| default | `kobako.wasm` | none — pure compute |
| regexp | `kobako+regexp.wasm` | ASCII Regexp / MatchData (B-41) |
| regexp-unicode | `kobako+regexp-unicode.wasm` | Regexp / MatchData with Unicode case-insensitive matching |
| json | `kobako+json.wasm` | JSON parse / generate (B-52) |
| full | `kobako+full.wasm` | ASCII Regexp + JSON |

The `regexp` and `regexp-unicode` variants differ by one behavior:
case-insensitive matching (`IGNORECASE` / `/i`) requires `regexp-unicode`; on
the plain `regexp` variant a case-insensitive pattern raises `RegexpError` at
compile, while the ASCII shorthand classes work on both. The `full` variant
composes the ASCII `regexp` capability with `json`.

## Naming

The default artifact name is fixed: `kobako.wasm` (N-4). A capability variant
adds a `+<cap>` suffix — `kobako+<cap>.wasm` — where `<cap>` names the opt-in
capability axis (`regexp`, `regexp-unicode`, `json`) or a composition shorthand
(`full` = ASCII regexp + JSON). The suffix encodes capability composition, not a
version; when a variant ships as a Release asset the version follows in the asset
filename (`kobako+<cap>-<version>.wasm`).

## Packaging policy

The published gem bundles exactly one Guest Binary: the pure default
`data/kobako.wasm` (→ [`SPEC.md`](../SPEC.md) § Code Organization, the gemspec
files whitelist). Capability variants are not bundled — they ship as GitHub
Release assets, or a developer builds one locally. This keeps the install
footprint minimal; a Host App that needs a capability downloads the matching
variant.

## Opt-in

A Host App selects a variant per Sandbox by constructing
`Kobako::Sandbox.new(wasm_path:)` pointed at the chosen binary. The capability
surface a guest sees is fixed by the binary it runs in; there is no runtime
capability negotiation.

## Build

`rake wasm:build` produces the default; `rake wasm:build:regexp`,
`wasm:build:regexp_unicode`, `wasm:build:json`, and `wasm:build:full` produce the
variants. Every variant — default and capability — passes through the canonical
boot bake (B-49); re-baking the same inputs yields a byte-identical artifact,
gated by the reproducible-build pipeline (F-10).
