# Benchmarks

Kobako maintains a regression benchmark suite: nine performance dimensions gated at release ([`support/roster.rb`](support/roster.rb)), four characterization suites, and the regexp variant profile. The table names each one.

| Dimension                     | Standing                       |
|-------------------------------|--------------------------------|
| Startup                       | gated                          |
| Transport round-trip          | gated                          |
| Codec                         | gated                          |
| mruby VM                      | gated                          |
| `Catalog::Handles`            | gated                          |
| Yield round-trip              | gated                          |
| `#preload` + `#run` dispatch  | gated                          |
| Dispatch glue                 | gated                          |
| Host per-invocation cost      | gated                          |
| Multi-thread                  | characterization               |
| `gvl:` scheduling             | characterization               |
| Per-Sandbox RSS               | characterization               |
| Guest-side setup scaling      | characterization               |
| Regexp                        | variant profile                |

The suite perceives drift against a fixed reference point, the committed anchor `benchmark/baseline.json`, rather than certifying a portable performance standard. Absolute numbers are meaningful only on hardware comparable to the machine that produced them. Per-release runs are archived under `benchmark/results/`. A cumulative +10 % regression past the anchor on any gated benchmark blocks release until a maintainer reviews or re-blesses.

## Measurement runners

The diagram traces a case from its benchmark file to the tables below, and the table says what each runner records.

```
   benchmark/<file>.rb            support/runner.rb              benchmark/results/
   ┌──────────────────┐         ┌──────────────────┐         ┌──────────────────┐
   │  bench.case(...) │ ──────▶ │  ips runner      │ ──────▶ │ <date>-<sha>.json│
   │  bench.case_with_│         │  case_with_usage │         │                  │
   │      usage(...)  │         │  one_shot runner │         └────────┬─────────┘
   │  bench.one_shot()│         │  wall-clock      │                  │
   └──────────────────┘         └──────────────────┘                  ▼
                                                          support/format_baseline.rb
                                                                      │
                                                                      ▼
                                                          Markdown tables (below)
```

| Runner            | When used                              | Records                                                            |
|-------------------|----------------------------------------|---------------------------------------------------------------------|
| `ips`             | iterated micro-benches                  | median `ips`, `ips_mean`, `ips_sd` per cycle                        |
| `case_with_usage` | sandbox-driven `ips` cases              | adds median `wall_time` + `memory_peak` from `Execution#usage` |
| `one_shot`        | cold paths, and costs outside the gate  | CPU seconds, one run or the median across `rounds`                  |
| wall-clock helper | multi-thread suite                      | wall seconds — CPU time would hide scheduler overhead               |

`one_shot` records a single run at `rounds: 1`, and the warm `1c`, `5b`, and `9a` windows take the median across `rounds`. `ips` is the median of per-cycle samples, because a GC-inflated cycle skews a mean but not a median. The arithmetic mean rides along as `ips_mean` for the capacity reading, mirroring Google Benchmark / Criterion.

For sandbox-driven cases, `case_with_usage` runs a dedicated post-measurement sampling loop, `UsageSampler`, bounded by a CPU budget. It reads the `Execution#usage` each invocation returns, so `wall_time` is the median of that distribution rather than a single point sample.

## Reading the numbers

The script `benchmark/support/format_baseline.rb` is the single source of truth for unit conversions. Regenerate the per-suite tables by re-running it.

```
bundle exec ruby benchmark/support/format_baseline.rb [path/to/results.json]
```

| Raw field                                | Derived          | Conversion                          |
|------------------------------------------|------------------|-------------------------------------|
| `ips` (≥ 1 000 000)                      | `ns` per op      | `1e9 / ips`                         |
| `ips` (1 000 .. 999 999)                 | `µs` per op      | `1e6 / ips`                         |
| `ips` (< 1 000)                          | `ms` per op      | `1000 / ips`                        |
| `ips_sd`                                 | `±sd` percentage | `(ips_sd / ips) * 100`              |
| `seconds` (`one_shot`)                   | `ms`             | `seconds * 1000`                    |
| `wall_time` (< 1e-6 s)                   | `ns`             | `wall_time * 1e9`                   |
| `wall_time` (1e-6 .. 1e-3 s)             | `µs`             | `wall_time * 1e6`                   |
| `wall_time` (≥ 1e-3 s)                   | `ms`             | `wall_time * 1e3`                   |
| `memory_peak` (< 1024 B)                 | `B`              | direct                              |
| `memory_peak` (1024 .. 1 048 575 B)      | `KiB`            | `memory_peak / 1024`                |
| `memory_peak` (≥ 1 048 576 B)            | `MiB`            | `memory_peak / 1 048 576`           |
| `rss_kb`                                 | `MB`             | `rss_kb / 1024`                     |
| `ops_per_sec` (concurrent)               | `ops/s`          | direct, with `k` suffix above 10 000 |

### Per-op arithmetic

Some rows hold more than one operation, so read them through the rule for their shape before comparing them with a per-op cost.

| Row shape                     | Example                                                  | Per-op cost                                    |
|-------------------------------|----------------------------------------------------------|------------------------------------------------|
| N ops in one invocation       | `2d-1000-calls-in-one-eval`                              | `total / N`, where the label carries `N`        |
| Batched one-shot              | `9a-100x-…`, `5b-alloc-1000-…`                           | `total / N`, where the label carries `N`        |
| Delta between waypoints       | `9e-run-replay-0-snippets` → `9e-run-replay-64-snippets` | subtract waypoints, divide by the count delta   |
| Batched waypoint row          | `9a-100x-…`                                              | divide by the batch, then take the waypoint delta |
| Sandbox-driven `ips` row      | also carries `wall_time` and `memory_peak`               | `1 / ips` − `wall_time` = per-`#eval` host wrapper |

Prose tables round to three significant figures, so treat sub-µs deltas as rounding noise. Consult the JSON for the precise value.

## Latest baseline

The anchor below produced every figure in this part, gated and characterization alike.

| Anchor field | Value                    |
|--------------|--------------------------|
| Commit       | `c1f97b8c`               |
| Captured     | 2026-09-15               |
| Machine      | macOS arm64, 16 CPUs     |
| Ruby         | 3.4.7, YJIT off          |

All figures come from that one `bench:all` round, so no row carries a capture stamp of its own. The shift against the previous anchor comes from this round's two changes. It fixed each Handle's Exposure when it is minted, and bounded an outbound value's nesting before the packer writes it. [Baseline changes](#baseline-changes) records that shift.

### Lifecycle & construction

These suites price cold-start, warm reuse, and per-request construction.

| Sub-section                | Covers                                       |
|----------------------------|----------------------------------------------|
| Sandbox construction       | cold-start and steady-state `Sandbox.new`     |
| Reuse versus construction  | warm reuse against per-request construction   |

#### Sandbox construction ([`cold_start.rb`](cold_start.rb))

Isolates Engine + Module JIT (one-time per process) from subsequent `Sandbox.new` (Engine + Module cached at process scope).

| Scenario                                                | Latency       |
|---------------------------------------------------------|---------------|
| First `Sandbox.new` in a fresh process (compiled-artifact disk cache warm) | **3.1 ms** |
| First `Sandbox.new` ever for a Guest Binary + gem version (cold disk cache) | ~500 ms, once per machine |
| Second-through-tenth `Sandbox.new` (cache warm)         | **~3 µs** each |
| Steady-state `Sandbox.new` only                         | **3.0 µs**    |
| Steady-state `Sandbox.new` + first `#eval("nil")`       | **59.5 µs**   |

The multi-hundred-millisecond Cranelift JIT now lands once per machine and gem version. The `.cwasm` disk cache ([`runtime.md`](../docs/spec/behavior/runtime.md)) carries the compiled artifact across processes. A fresh process therefore deserializes in single-digit milliseconds instead of recompiling. The cold-cache figure is the fresh-process cost from before the disk cache existed; the suite no longer measures it.

#### Reuse versus construction

The table sets reusing one Sandbox against constructing one per request.

| Pattern                                                          | Cost per request | Source                  |
|------------------------------------------------------------------|------------------|-------------------------|
| Reuse the same Sandbox (a Service call on a warm instance)       | **69.7 µs**      | `2a-empty-call`         |
| Fresh Sandbox every request (`Kobako::Sandbox.new.eval("nil")`)  | **59.5 µs**      | `1b-sandbox-new+eval-nil` |
| Overhead of constructing a new Sandbox per request               | **3.0 µs**       | `1a-sandbox-new`        |

Construction is no longer a term worth reasoning about: it is about 3 µs, an order of magnitude below the invocation it precedes. That is why the two rows above do not order the way the reuse-or-construct question implies. They run different guest work, not the same work with and without a constructor.

The wasm instance is created per invocation either way ([`mruby.md`](../docs/spec/behavior/mruby.md)), so a fresh Sandbox pays no per-instance boot. `wall_time` reads the guest export only. Total minus `wall_time` (2a: 69.7 − 36.8 ≈ 33 µs) bundles the per-invocation instantiation with the host wrapper. It is therefore not a single-digit-µs wrapper readout.

### Wire layer

These suites cover each crossing of the wire and the codec on each side.

| Sub-section        | Direction                 |
|--------------------|---------------------------|
| Transport latency  | guest→host Transport      |
| Host-side codec    | host codec                |
| Guest-side codec   | guest codec               |
| Yield latency      | host→guest yield          |

#### Transport latency ([`transport_roundtrip.rb`](transport_roundtrip.rb))

One guest→host Service call wrapped in one `#eval`. Each row bundles `#eval` setup (~70 µs) with the round-trip; cross-row deltas isolate the round-trip contribution. Per-call steady state is read from `2d`'s `wall_time / 1000`.

| Scenario                                                   | Latency                                | `wall_time` (guest)               |
|------------------------------------------------------------|----------------------------------------|------------------------------------|
| One Service call returning `nil`, alone in `#eval`         | **69.7 µs**                            | 36.8 µs                            |
| One Service call with one Integer arg                      | **70.0 µs**                            | 37.8 µs                            |
| One Service call with one Symbol-keyed keyword arg         | 72.2 µs                                | 39.2 µs                            |
| 1 000 sequential Service calls inside one `#eval`          | 5.81 ms total → **5.8 µs per call**    | 5.80 ms / 5.8 µs per call          |
| Handle chain — one call returns object, second targets the Handle | 87.6 µs | 52.1 µs |

#### Host-side codec ([`codec.rb`](codec.rb))

`Kobako::Codec` encode / decode directly from Ruby — no wasm boundary. Characterizes the host codec on its own; the per-wire-type table fixes one entry per value-carrying row of [`docs/wire/payload-msgpack.md`](../docs/wire/payload-msgpack.md) § Type Mapping.

| Payload                                  | Encode  | Decode  |
|------------------------------------------|---------|---------|
| String, 64 B                             | 335 ns  | 427 ns  |
| String, 1 KiB                            | 442 ns  | 486 ns  |
| String, 64 KiB                           | 8.7 µs  | 2.7 µs  |
| String, 1 MiB                            | 60.3 µs | 35.8 µs |
| Array nested 1 deep (1 KiB leaf)         | 532 ns  | 654 ns  |
| Array nested 64 deep (1 KiB leaf)        | 930 ns  | 8.5 µs  |

| Wire type                                | Encode      | Decode      |
|------------------------------------------|-------------|-------------|
| `nil` / Boolean / Integer / Float        | 333-340 ns  | 360-368 ns  |
| Short String / binary String             | 339 ns      | 385-411 ns  |
| 3-element Array / 1-entry Hash           | 351-370 ns  | 610-638 ns  |
| Symbol (ext 0x00)                        | 467 ns      | 611 ns      |
| Handle (ext 0x01)                        | 511 ns      | 934 ns      |

The capture read two encode rows slow, `3a-host-encode-64KiB` and `3b-host-encode-depth-1`. [Baseline changes](#baseline-changes) records them with their arbitration.

#### Guest-side codec ([`codec.rb`](codec.rb))

Guest builds a value in mruby and returns it from `#eval`. `wall_time` isolates "guest export inside wasmtime". The total additionally carries the per-invocation instantiation + host wrapper (payload decode + outcome decode + capture readout), ~34 µs on small payloads. Size scaling therefore lives inside `wall_time`.

| Guest script returns                          | Latency  | `wall_time` (guest) |
|-----------------------------------------------|----------|---------------------|
| `"x" * 64` (64 B String)                      | 58.6 µs  | 24.5 µs             |
| `"x" * 1024` (1 KiB String)                   | 58.4 µs  | 25.0 µs             |
| `"x" * 65536` (64 KiB String)                 | 89.7 µs  | 44.6 µs             |
| `"x" * 524288` (512 KiB String)               | 303.1 µs | 154.4 µs            |
| Array nested 1 deep (1 KiB leaf)              | 58.3 µs  | 24.5 µs             |
| Array nested 64 deep (1 KiB leaf)             | 92.9 µs  | 45.7 µs             |

Note: mruby caps a single String at 1 MiB; the largest guest sample here is 512 KiB. Composite values can still approach the 16 MiB wire payload limit.

#### Yield latency ([`yield_roundtrip.rb`](yield_roundtrip.rb))

Host-initiated counterpart of #2 — a Service method `yield`s into a guest-supplied block. The cost lives on a different path (Yield Reply codec, `yield` downcall, guest `BLOCK_STACK`), so a regression here is invisible to #2. Per-yield steady state is `6c` `wall_time / 1000`.

| Case                            | What it isolates                                                                          |
|---------------------------------|--------------------------------------------------------------------------------------------|
| `6a-single-yield`               | One yield (tag 0x01 ok) above the no-block #2 baseline.                                    |
| `6b-block-no-yield`             | `block_given` flag travels, Yielder built, never invoked — re-entry-free floor. |
| `6c-1000-yields-in-one-call`    | 1 000 yields in one dispatch — load-bearing for `each`-style Services.        |
| `6d-yield-break`                | Block runs `break` on first yield (tag 0x02), unwinding via catch/throw. |

| Case                            | Latency                            | `wall_time` (guest)        |
|---------------------------------|------------------------------------|----------------------------|
| `6a-single-yield`               | 73.3 µs                            | 41.6 µs                    |
| `6b-block-no-yield`             | 67.9 µs                            | 39.2 µs                    |
| `6c-1000-yields-in-one-call`    | 3.34 ms → **3.3 µs per yield**     | 3.21 ms / 3.2 µs per yield |
| `6d-yield-break`                | 196.3 µs                           | 160.2 µs                   |

`6c` gates on `wall_time` so the 1 000-element host-side decode is excluded from the gated metric.

### mruby VM & Handles

These suites cover pure interpreter work and the host-side ID→object table.

| Sub-section   | Covers                          |
|---------------|---------------------------------|
| mruby VM      | pure interpreter work           |
| Handle table  | the host-side ID→object table   |

#### mruby VM ([`mruby_eval.rb`](mruby_eval.rb))

Self-contained mruby computations with no Transport, whose only host cost is the constant `Sandbox#eval` overhead. Regression signal for `build_config/wasi.rb` flag changes and the IO write path.

| Script                                                        | Latency                                          | `wall_time` (guest)       |
|---------------------------------------------------------------|--------------------------------------------------|---------------------------|
| 100 000-iteration integer XOR loop                            | **36.65 ms**                                     | 36.53 ms                  |
| 1 000 single-character String appends                         | 452 µs                                           | 412 µs                    |
| 100 cycles of `raise` / `rescue`                              | 199 µs → 2.0 µs per cycle                        | 166 µs                    |
| 1 000 `puts` of 64 B (below 1 MiB stdout cap)                 | 2.62 ms → 2.6 µs per write                       | 2.55 ms                   |
| 2 048 `puts` of ~1 KiB against the 1 MiB stdout cap           | 5.57 ms (first ~1 024 land, rest silently dropped) | 5.27 ms                 |

`4e` holds at the 64 KiB per-invocation memory floor and `4f` reaches only 192 KiB, both far below the bytes written. That confirms the IO write path is wasi-libc-bound, not guest-linear-memory-bound. `stdout_truncated?` flips to `true` after `4f`.

#### Handle table ([`catalog_handles.rb`](catalog_handles.rb))

`Catalog::Handles` is the host-side ID→object mapping, reset at the start of every invocation. The 1 K → 1 M waypoint rows verify the underlying dictionary stays O(1) as it grows. The `5c` row deliberately measures `#eval` cost under sustained heap pressure that `1b` cannot detect.

| Scenario                                                            | Latency                  |
|---------------------------------------------------------------------|--------------------------|
| Allocate one Handle in an empty table                               | 2.5 µs                   |
| Allocate 100 Handles from empty                                     | 158.1 µs total           |
| Allocate 10 000 Handles from empty                                  | 18.70 ms total           |
| Allocate 100 000 Handles from empty                                 | 191.12 ms total          |
| 1 000 allocs against a 1 K-entry table                              | 1.442 ms                 |
| 1 000 allocs against a 10 K-entry table                             | 1.516 ms                 |
| 1 000 allocs against a 100 K-entry table                            | 1.663 ms                 |
| 1 000 allocs against a 1 M-entry table                              | 2.484 ms                 |
| Warm `#eval("nil")` under sustained heap pressure (1 M-entry table) | 147.4 µs (`wall_time` = 23.7 µs) |

Per-alloc cost holds 1.4–1.7 µs from 1 K to 100 K entries and reaches 2.5 µs at 1 M. The lookup therefore stays constant-time, and most of that level is the Exposure each alloc fixes. Each entry retains its Exposure, so a large table is also a heavier heap. That is what `5c` now shows: its guest `wall_time` holds while the total around it grows. The counter is capped at `0x7fff_ffff`; the cap guard is constant-time and not iterated here.

### Host side, isolated

These suites separate the two halves of an invocation.

| Sub-section          | Measures                                               |
|----------------------|--------------------------------------------------------|
| Dispatch-glue isolation | host spend, with guest execution out of the window  |
| Host invocation      | host spend, with guest execution out of the window     |
| Guest-side setup     | the guest-side setup the other two leave out of frame  |

Everything above either includes guest execution in its total or gates on `wall_time`, which is the guest export alone. A change confined to the host half therefore moves neither.

#### Dispatch-glue isolation ([`dispatch_glue.rb`](dispatch_glue.rb))

This suite is the predictive half of the GVL-impact toolkit; the Multi-Thread suite (#7) is the confirmation half. It calls `Kobako::Transport::Dispatcher.dispatch` directly with a routed `Transport::Call`, with no wasm, boundary, or guest codec in the window. That isolates `G`, the host glue of one guest→host dispatch:

1. Decode the payload.
2. Resolve the target.
3. Invoke.
4. Encode the reply body.

The driver frames the core envelope, decoding it and encoding the Reply *outside* the GVL re-acquisition. That framing parallelizes, so it is deliberately out of frame here.

| Case                          | `G` per dispatch | What it isolates                                     |
|-------------------------------|------------------|------------------------------------------------------|
| `10a-empty-call`              | 2.34 µs          | Floor: decode the payload + path lookup + invoke + encode a nil reply |
| `10b-primitive-arg`           | 2.45 µs          | + one Integer arg                                    |
| `10c-kwargs`                  | 3.24 µs          | + Symbol-keyed kwargs (ext 0x00)                     |
| `10d-small-return-16`         | 3.12 µs          | Service returns a 16-element Array                   |
| `10e-large-return-256`        | 12.5 µs          | 256-element Array — `G` grows with returned payload  |

`G` grows with the returned payload on two counts: encoding it, and first measuring it against the wire's nesting bound. The measure costs roughly 12–15 ns per returned element, the cost of refusing a value the packer could not finish.

##### Amdahl bound

`gvl: :release` parallelizes everything except `G`. The block shows how `G` bounds the multi-core speedup ceiling.

```
d = k·G / T                 invocation doing k dispatches in wall-time T
d ≈ 2.34 / 5.80 ≈ 0.40      glue share of a 2d roundtrip, the per-dispatch floor
ceiling ≈ 2.5×              pure-dispatch workload, rising toward N× with compute
```

So the ceiling for an invocation doing `k` dispatches in wall-time `T` is Amdahl-bounded by `d = k·G / T`. The Services are pure-CPU on purpose. A Service doing real I/O releases the GVL during the syscall, so its wait already overlaps today and must not count toward `G`.

Compose with the full roundtrip (`transport_roundtrip` `2d` ≈ 5.80 µs/call) for the per-dispatch floor of `d`. Glue is 2.34 µs of a 5.80 µs roundtrip, so `d ≈ 0.40`, since the remaining ~60 % (guest codec + boundary) parallelizes. That gives a pure-dispatch workload a ~2.5× multi-core ceiling, which rises toward `N×` as compute per invocation grows.

##### GVL handoff

The model prices the serialized glue but not the GVL handoff that reaching it costs. The gvl suite measures ~0.5× on a dispatch-heavy shape, so read `d` by workload.

| Workload        | Reading of `d`                                  |
|-----------------|-------------------------------------------------|
| dispatch-bound  | a ceiling the workload stays well under         |
| compute end     | where the ceiling is actually approached        |

`G` is the gem-controlled glue floor only. A Service's own Ruby CPU is the Host App's to measure, so the gem publishes `G` and the method, never a single `d`.

#### Host invocation ([`host_invocation.rb`](host_invocation.rb))

Driven against `test/fixtures/minimal_null_guest.wat`, a guest that satisfies the invocation ABI and does nothing else. The total therefore *is* the host's cost rather than a total minus a guest budget. That subtraction is why this suite exists. On the thousand-call rows it is a difference of two near-equal measured milliseconds. A single round read +326 % on `2d` against −94 % on the neighbouring `2f`.

| Case                          | Cost per invocation | What it adds                                       |
|-------------------------------|---------------------|-----------------------------------------------------|
| `12a-eval`                    | 18.2 µs             | The floor every invocation pays                     |
| `12b-run-no-args`             | 22.5 µs             | + 4.3 µs for the `#run` envelope                    |
| `12c-run-args`                | 25.1 µs             | + 2.6 µs for the payload codec's argument encoding |
| `12d-eval-8-bound-services`   | 19.7 µs             | + 1.5 µs for the preamble eight bound Services add  |

Read `12a` against `2a-empty-call` (69.7 µs total): roughly a quarter of a minimal round-trip is host-side work outside the guest export. `12d` is the host half of what a registry costs each invocation, and it is nearly free. The guest half, materializing each binding into the `mrb_state`, is out of frame by construction. That is the point: the two were previously only measurable together, and only their sum was known.

#### Guest-side setup ([`guest_setup.rb`](guest_setup.rb))

The complement of `#12`: the two per-invocation setup costs that grow with something a Host App controls, measured against the real Guest Binary. `13a` sweeps statement count to expose mruby's compile cost as source grows. A statement guarded by `if false` costs the same as one that runs. The axis is therefore statement count, not source length or work done. `13b` varies the binding path shape, covered under [Binding path shapes](#binding-path-shapes).

Both were unmetered before, so neither had an anchor to compare a change against. `#4` holds its script fixed and `#12` drives the null guest.

| Axis                                  | `wall_time` (guest) | Marginal cost                     |
|---------------------------------------|---------------------|-----------------------------------|
| 1 statement                           | 26.6 µs             | the invocation floor              |
| 20 statements                         | 30.4 µs             | ~0.20 µs per statement            |
| 100 statements                        | 49.8 µs             | ~0.24 µs per statement            |
| 400 statements                        | 154.9 µs            | ~0.35 µs per statement            |
| 0 bindings                            | 23.7 µs             | the shape-independent floor       |
| 32 bindings, top-level names          | 59.2 µs             | ~1.11 µs per binding              |
| 32 bindings, one shared namespace     | 52.5 µs             | ~0.90 µs per binding              |
| 32 bindings, a namespace each         | 74.0 µs             | ~1.57 µs per binding              |

Compile cost is roughly linear to 100 statements and steepens past it.

##### Binding path shapes

`13b` fixes the binding count and varies the path shape, which is the guest half `12d` leaves out of frame. The table says what each shape pays for.

| Path shape           | Pays for                                            |
|----------------------|-----------------------------------------------------|
| top-level name       | the leaf alone                                      |
| shared namespace     | one prefix resolution, amortised across the group   |
| per-path namespace   | its own prefix resolution                           |

Binding materialisation is paid **per invocation, not per Sandbox**. Its per-path cost tracks how much namespace each path has to resolve. A registry declared under one shared namespace therefore costs less per entry than one where every path brings its own. The top-level and shared-namespace rows sit within each other's spread (±8 % on `wall_time`) and have swapped order between anchors. Only the per-path row reads as separated from them.

### Setup-once dispatch

#### Preload dispatch ([`preload_dispatch.rb`](preload_dispatch.rb))

`#preload(code:)` registers snippets that replay against the canonical boot state on every invocation; `#run(:Target)` dispatches into a preloaded entrypoint. The rows isolate each verb's contribution via waypoint deltas.

```
   9a sweep:  100x (Sandbox.new + 1 / 8 / 64 #preload)  ─▶ ÷100, then delta / Δsnippets
   9e sweep:  warm #run with 0 / 8 / 64 snippets        ─▶ delta / Δsnippets ≈ 7.0 µs per snippet replay
```

| Scenario                                                            | Latency  | `wall_time` (guest) |
|---------------------------------------------------------------------|----------|---------------------|
| 100 × (`Sandbox.new` + 1 `#preload(code:)`)                         | 0.370 ms → 3.7 µs each | —         |
| 100 × (`Sandbox.new` + 8 `#preload(code:)`)                         | 1.185 ms → 11.9 µs each | —        |
| 100 × (`Sandbox.new` + 64 `#preload(code:)`)                        | 13.745 ms → 137 µs each | —        |
| Warm `#run(:Noop)` (1 entrypoint preloaded)                         | 78.6 µs  | 37.9 µs             |
| Warm `#run(:Echo, 42)` (positional arg)                             | 77.3 µs  | 37.4 µs             |
| Warm `#run(:Greet, name: :alice)` (Symbol-keyed kwargs)             | 83.6 µs  | 41.5 µs             |
| Warm `#run(:Wrap, StringIO)` (host→guest auto-wrap) | 81.5 µs  | 29.5 µs             |
| Warm `#run(:Noop)` with 0 helper snippets preloaded                 | 69.7 µs  | 30.5 µs             |
| Warm `#run(:Noop)` with 8 helper snippets preloaded                 | 112.5 µs | 71.2 µs             |
| Warm `#run(:Noop)` with 64 helper snippets preloaded                | 548.6 µs | 479.3 µs            |

`9a` rows carry no `wall_time`: the timer wraps `Sandbox.new + #preload` and neither calls the guest export. Registration is paid once per Sandbox rather than once per invocation, so it is characterized rather than gated. It records CPU seconds for a batch of 100 under a label that names the batch. One `Sandbox.new` plus one `#preload` is a few µs against a 1 µs clock granularity, which no median over single observations recovers. A `deep_wrap` / `Catalog::Handles#alloc` super-linear regression would show as `9f` rising above `9c`.

Snippet replay is the cost that does not amortize: `(479.3 − 30.5) / 64 ≈ 7.0 µs` of guest budget on **every** invocation, per preloaded snippet. It is the one figure here a Host App scales by a number it chooses.

### Ungated characterization

#### Multi-Thread behavior ([`concurrent/threads.rb`](concurrent/threads.rb))

Captured under the default `gvl: :hold`, where wasm-side work is GVL-serialized and only Ruby-side `#eval` setup overlaps; the gvl suite below measures what `:release` changes. Wall-clock timing because that is where scheduler effects manifest.

| Scenario                                                           | Result          |
|--------------------------------------------------------------------|-----------------|
| 1 Thread, owning one Sandbox                                       | 15.9k `#eval`/s |
| 2 Threads, each owning one Sandbox                                 | 16.2k `#eval`/s |
| 4 Threads, each owning one Sandbox                                 | 14.5k `#eval`/s |
| 8 Threads, each owning one Sandbox                                 | 15.9k `#eval`/s |
| Per-Sandbox `Sandbox.new` cost, single-Threaded                    | 0.072 ms        |
| Per-Sandbox `Sandbox.new` cost, 8 Threads in parallel              | 0.030 ms each (0.237 ms total / 8) |
| `#eval("nil")` baseline                                            | 0.055 ms        |
| `#eval("nil")` while another Thread is in a long `#eval`           | 0.086 ms (1.56× baseline) |

Throughput stays flat across Thread counts, which is the `:hold` signature — the GVL, not the Sandbox count, is the bound. A long-running script still does not block other Threads' short `#eval` calls by hundreds of ms. Host-side synchronization yields the GVL and the contending Thread interleaves. That contention ratio swings run to run with scheduler quirks; the order of magnitude is the regression signal, not the multiple.

#### GVL scheduling ([`concurrent/gvl_scheduling.rb`](concurrent/gvl_scheduling.rb))

What the per-Sandbox `gvl:` mode buys and costs, bracketed by two opposed workloads plus an arm where every Thread shares one Sandbox. Weak scaling applies: each Thread does a fixed amount of work. Under perfect parallelism the `:release` column stays flat as N grows while `:hold` climbs with it. Wall-clock, not CPU time: parallel progress is exactly what a CPU-time sum cannot see.

| Threads | compute (hold → release) | dispatch (hold → release) | compute, one shared Sandbox |
|---------|--------------------------|---------------------------|-----------------------------|
| 1       | 331 → 329 ms (1.01×)     | 27.8 → 25.0 ms (1.11×)    | 330 → 330 ms (1.00×)        |
| 2       | 660 → 336 ms (1.97×)     | 46.1 → 101 ms (0.46×)     | 660 → 341 ms (1.94×)        |
| 4       | 1318 → 344 ms (3.83×)    | 93.8 → 176 ms (0.53×)     | 1334 → 343 ms (3.89×)       |
| 8       | 2640 → 350 ms (7.55×)    | 192 → 402 ms (0.48×)      | 2706 → 348 ms (7.78×)       |

Three readings follow:

1. The compute `:release` column moves 329 → 350 ms from 1 to 8 Threads. Guest compute parallelizes near-perfectly once the GVL is out of the way.
2. The dispatch arm settles at a stable ~0.5× from 2 Threads up. Every dispatch re-acquires the GVL, and that handoff costs more than the released span saves. That makes `:release` a net loss for dispatch-heavy work.
3. The shared-Sandbox arm tracks the distinct-Sandbox one to within a few percent, confirming that sharing a Sandbox costs no parallelism.

#### Memory cost ([`memory.rb`](memory.rb))

The suite reads two lenses. External RSS sampling (`ps -o rss=`) never reaches inside the Sandbox's mruby heap. The `memory_peak` reader reports the invocation's `memory.grow` delta in guest linear memory. Together they give the granularity that capacity planning needs without per-invocation instrumentation, which kobako does not offer.

| Scenario                                                              | RSS                                                                            | `memory_peak`                |
|-----------------------------------------------------------------------|--------------------------------------------------------------------------------|------------------------------|
| Process RSS at boot (no Sandbox)                                      | 27.6 MB                                                                        | —                            |
| RSS after the first `Sandbox.new` + `#eval("nil")`                    | 33.3 MB (**+5.8 MB** — Engine init + `.cwasm` deserialize, one-time)           | —                            |
| RSS after 10 Sandboxes total                                          | 33.3 MB (+16 KB — one page — over the first)                                   | —                            |
| RSS after 100 Sandboxes total                                         | 33.3 MB (<1 KB per additional Sandbox)                                         | —                            |
| RSS after 1 000 Sandboxes total                                       | 33.7 MB (~**0.4 KB per additional Sandbox**)                                   | —                            |
| RSS drift after 10 000 consecutive `#eval("nil")` on one Sandbox      | +2.9 MB, flat from ~8 000 onward                                               | **64 KiB** per invocation (one `memory.grow` above the baked image) |
| Peak RSS while holding a 512 KiB return value                         | +1.5 MB above baseline                                                         | **1.6 MiB** guest `memory.grow` |
| Retained RSS after GC of the same value                               | +1.5 MB (allocator does not eagerly return pages to the OS)                    | —                            |
| Peak RSS while holding a 1 MiB capped stdout buffer                   | +3.2 MB above baseline (allocator-state-dependent)                             | **192 KiB** (stdout flows via WASI pipe, not linear memory) |
| Retained RSS after GC of the same capture                             | +3.2 MB                                                                        | —                            |

Budget ~34 MB up front per worker process. An idle Sandbox holds no wasm instance ([`mruby.md`](../docs/spec/behavior/mruby.md)), so additional Sandboxes cost KB, not MB: **1 000 tenants ≈ 33.7 MB** in one Ruby process. Per-invocation linear memory lives and dies with the invocation's instance. RSS figures swing with host load and allocator state, so treat them as ranges.

#### Regexp engine ([`regexp.rb`](regexp.rb))

Regexp is an opt-in capability gem, excluded from the gated default binary. Suite #11 therefore runs against the `+regexp-unicode` variant and never blocks release. Each row is a 1 000-iteration loop over a 25-byte subject.

| Scenario                                                   | Throughput | Per op           |
|------------------------------------------------------------|------------|------------------|
| `=~` literal in a loop (recompiles each iteration)         | 173 i/s    | 5.8 µs / match   |
| `=~` hoisted (compiled once)                               | 196 i/s    | 5.1 µs / match   |
| `match?` hoisted                                           | 994 i/s    | 1.0 µs / match   |
| `Regexp.compile` ×1 000, no match                          | 903 i/s    | 1.1 µs / compile |
| empty 1 000-loop (overhead only)                           | 2.74k i/s  | 0.4 µs           |
| capturing `match`                                          | 175 i/s    | 5.7 µs / match   |
| `scan` every word of a sentence                            | 206 i/s    | 4.8 µs / scan    |
| `gsub` upcasing every word (block)                         | 23 i/s     | 43 µs / gsub     |
| `split` on a delimiter pattern                             | 330 i/s    | 3.0 µs / split   |

`=~` costs ~5× `match?` because it eagerly builds the `MatchData` and refreshes the match globals every call. `match?` skips both, so reach for `match?` for boolean tests. The literal-in-loop vs hoisted gap stays small because the per-invocation compile cache ([`regexp.md`](../docs/spec/behavior/regexp.md)) absorbs mruby's recompile-per-literal.

## Baseline changes

This part diffs against the immediately previous baseline only; pre-history lives in `benchmark/results/<date>-<sha>.json`.

| Baseline | Commit       | Date       | Round                                                         |
|----------|--------------|------------|---------------------------------------------------------------|
| Previous | `ff677065`   | 2026-07-31 | 0.21.1: cached the ABI probe; `#run` stopped rebuilding its constant baseline |
| This     | `c1f97b8c`   | 2026-09-15 | 0.26.0: Exposure fixed per alloc; outbound nesting bounded     |

In the 0.26.0 round every `Handles#alloc` fixes the object's Exposure, so the surface a guest reaches defaults to deny. A Service answer or a yield argument is measured against the wire's nesting bound before the packer writes it.

### Metric deltas

#### Accepted steps

Two steps are accepted as the cost of what the round refuses. The table lists them with the rows that carry them.

| Row                          | Previous → this                          | Reading                         |
|------------------------------|------------------------------------------|---------------------------------|
| `catalog_handles` `5a-*`     | ips −66 % to −74 %, +1.1–1.8 µs per alloc | the Exposure each alloc fixes  |
| `5b` batches                 | 0.44–0.57 → 1.44–2.48 ms per 1 000 allocs | the same step                  |
| `5c` total                   | 61.3 → 147.4 µs                          | guest `wall_time` holds         |
| `2e-handle-chain`            | 48.3 → 52.1 µs                           | within its band                 |
| `dispatch_glue` `10e`        | 9.41 → 12.5 µs                           | the nesting measure             |
| `dispatch_glue` `10d`        | +5.5 %                                   | within its band                 |

The Exposure each alloc fixes is its class's own surface, enumerated once per class per table, plus a per-object singleton and predicate check. `5c` rises because every entry retains its Exposure, and a 1 M-entry table is that much more heap. Inside a whole invocation the step stays within its band. `2e-handle-chain` mints and calls through a Handle, and no `dispatch_glue` row leaves its band, though each checks an Exposure. A surface cache that outlives one invocation's table is the lever if the alloc cost ever matters on its own.

Measuring an answer's nesting walks every returned member that could be a container, roughly 12–15 ns per element. It is what lets the host refuse a value the packer would otherwise recurse into without end.

#### Unattributed improvements

These rows improved, and the round does not attribute them.

| Row                                   | Shift                        |
|---------------------------------------|------------------------------|
| `mruby_eval` `4a`                     | −12.8 % on guest `wall_time` |
| `mruby_eval` `4b`                     | −11.1 % on guest `wall_time` |
| `mruby_eval` `4c`                     | −7.5 % on guest `wall_time`  |
| `guest_setup` `13a-eval-statements-400` | 181.6 → 154.9 µs           |
| `host_invocation` `12b`               | −10.3 %                      |

The round spans the wasmtime 48 and beni 0.14 upgrades among other changes on both sides. No paired measurement separates them.

#### Arbitrated flag

One flag was arbitrated and not accepted as a cost. The table sets each slow encode row against its isolated repeats.

| Row                          | Blessed reading                         | Isolated repeats                       |
|------------------------------|-----------------------------------------|----------------------------------------|
| `codec/3b-host-encode-depth-1` | 532 ns; +13.3 % against a ±12.8 % band | +2.4 / −2.4 / −4.6 % vs previous anchor |
| `3a-host-encode-64KiB`       | 8.7 µs; absorbed by its archive band    | 7.1 µs                                 |

The encoder `3b` calls gained only a comment in this round. The nesting measure sits in the dispatcher and the yielder, outside its window. The shape argues against code too: depth 4, 16, and 64, each a superset of depth 1's work, read about 7 % faster. Three isolated repeats of the suite ran on the same build, each writing to its own results directory, and straddled the previous anchor. The blessed 532 ns is therefore slow for this row, as is the 8.7 µs of `3a`. Both budgets are more generous than they should be until the next round moves them.

#### Characterization shifts

These shifts are accepted as within range without arbitration.

| Row                                              | Previous → this  | Note                       |
|--------------------------------------------------|------------------|----------------------------|
| First `Sandbox.new` in a fresh process           | 1.3 → 3.1 ms     | disk cache warm; one ungated observation |
| Single-Threaded `Sandbox.new`, multi-Thread suite | 0.034 → 0.072 ms | —                         |
| RSS drift across 10 000 `#eval("nil")`           | +1.5 → +2.9 MB   | —                          |

### Roster / schema

The roster and the measurement methods are unchanged since `ff677065`.

| Change               | Since `ff677065` |
|----------------------|------------------|
| Cases added          | none             |
| Cases dropped        | none             |
| Method versions      | unchanged        |

Every suite ran under the method it ran under at `ff677065`. The whole gated set was therefore judged against the previous anchor.

## Running

The block lists every entry point; each comment says what the task adds.

```bash
bundle exec rake bench                   # every gated benchmark (CI-friendly, payloads ≤ 1 MiB)
bundle exec rake bench:full              # adds the 16 MiB codec payload sweep
bundle exec rake bench:concurrent        # multi-Thread characterization (#7)
bundle exec rake bench:gvl_scheduling    # gvl: hold-vs-release wall-clock scaling
bundle exec rake bench:memory            # per-Sandbox RSS characterization (#8)
bundle exec rake bench:preload_dispatch  # #preload + #run dispatch on its own (gated #9; bench runs it too)
bundle exec rake bench:dispatch_glue     # dispatch-glue isolation on its own (gated #10; bench runs it too)
bundle exec rake bench:host_invocation   # host per-invocation cost against the null guest (gated #12; bench runs it too)
bundle exec rake bench:guest_setup       # guest-side compile + binding scaling characterization (#13)
bundle exec rake bench:regexp            # regexp characterization on the +regexp-unicode variant (#11)
bundle exec rake bench:all               # whole-round sweep: bench:full + every characterization
bundle exec rake "bench:report[head.json,base.json]"  # Markdown head-vs-base summary (what CI posts on a PR)
```

Which suites are gated and which are characterization is `benchmark/support/roster.rb`'s to say; `rake -T bench` is the full task catalog. Each rake task shells out to `bundle exec ruby benchmark/<file>.rb`; invoke a single script directly for fast iteration. `bundle exec rake bench` runs in 5-8 min on a current-gen laptop, and each characterization task adds 30 s to 1 min. Codec dominates, holding more cases than the rest of the gated set together.

YJIT is not turned on by the suite. Use `RUBY_YJIT_ENABLE=1 bundle exec rake bench` or `--yjit` to capture a YJIT baseline. The resulting JSON records `yjit_enabled: true` so it is unambiguously distinct.

## Result files

Every run writes (or merges into) `benchmark/results/<date>-<short-sha>.json`. The probes of one round share that file. A round that merges into a file an earlier one left behind re-stamps `env`. The machine state a file reports therefore belongs to the round its newest suites were captured in, not to whichever round created the file:

```json
{
  "env": {
    "ruby_version": "3.4.7",
    "ruby_platform": "arm64-darwin24",
    "processors": 16,
    "yjit_enabled": false,
    "git_sha": "711665d",
    "captured_at": "2026-05-27T14:20:41Z",
    "load_avg": 2.41,
    "power_source": "ac",
    "cpu_probe_spread_pct": 0.44
  },
  "suites": {
    "cold_start":          [ { "label": "1a-sandbox-new", "ips": 8013.0, "ips_mean": 7956.4, "ips_sd": 112, "iterations": 18432, "cycles": 3 } ],
    "transport_roundtrip": [ { "label": "2a-empty-call",  "ips": 7365.3, "ips_mean": 7357.6, "ips_sd": 15,  "iterations": 24576, "cycles": 3,
                               "wall_time": 0.0001273, "wall_time_sd": 0.0000191, "wall_time_samples": 47,
                               "memory_peak": 0 } ],
    ...
  },
  "methods": { "cold_start": 1, "transport_roundtrip": 1, "codec": 2 }
}
```

| Field                        | Meaning                                                                    |
|------------------------------|----------------------------------------------------------------------------|
| `ips`                        | Median of per-cycle iterations / CPU second; higher is better.             |
| `ips_mean`                   | Arithmetic mean of the per-cycle `ips` samples.                            |
| `ips_sd`                     | Standard deviation of the per-cycle `ips` samples.                         |
| `iterations` / `cycles`      | Total iterations measured and number of samples collected within the time budget. |
| `seconds` / `rounds`         | `one_shot` CPU seconds (median across `rounds` when > 1); multi-thread wall seconds |
| `env.load_avg`               | 1-minute load at capture                                                   |
| `env.power_source`           | AC vs battery at capture                                                   |
| `env.cpu_probe_spread_pct`   | Spread across two back-to-back runs of a fixed pure-CPU probe              |
| `wall_time`                  | Median of `Execution#usage` samples                                        |
| `wall_time_sd`               | Deviation across those samples                                             |
| `wall_time_samples`          | How many samples there were                                                |
| `memory_peak`                | `memory.grow` delta past the per-invocation baseline                       |
| `methods`                    | Measurement-method version each captured suite ran under                   |

`env.cpu_probe_spread_pct` is the session's own noise floor. The `wall_time` and `memory_peak` fields appear on sandbox-driven rows only. A gated row is always sampled: a single observation leaves the noise band nothing to read, so the `+10 %` floor would be its only bar. The `methods` map is the key that says which archived runs a figure may be compared against.

A run says which release it belongs to through its `git_sha`. Its `methods` map says which measurements it may be compared against, so the archive needs no second index.

## Release gate

`rake bench:gate[current,baseline]` compares a run against the committed anchor `benchmark/baseline.json`. It exits non-zero on either a gated case regressed past the anchor or a gated case the anchor does not yet cover. The table says where each part of the gate lives.

| Part                               | Role                                 |
|------------------------------------|--------------------------------------|
| `benchmark/support/comparator.rb`  | the judgment itself                  |
| `gate.rb`                          | resolves the pair, prints, and aborts |
| `rake test:bench`                  | runs both in the test suite          |

A case is flagged only when its regression past the anchor clears both a +10 % floor and a noise band. The floor is cumulative against the anchor, not the previous run. The band can only widen the bar on high-variance rows, never narrow it below the floor.

The gate is stage 1, a smoke detector against the anchor. A flag is a reason to arbitrate, not yet a verdict; [Stage 2 arbitration](#stage-2-arbitration) is the judge.

### Noise band

The band is the wider of two dispersions, because they see different noise.

| Dispersion | Source | Sees |
|------------|--------|------|
| Within-run | Standard error of the median the row records | GC and per-cycle jitter |
| Between-run | Median relative move between consecutive archived runs, over the last 10 (`benchmark/support/history.rb`) | Allocator state and frequency scaling across processes |

The within-run half scales the row's `ips_sd` / `wall_time_sd` to the `cycles` / `wall_time_samples` that median came from.

#### Within-run half

The within-run half guards a median, so it scales the recorded deviation to the sample count behind it. Read whole, that deviation is the spread of a *single* observation. The widest row in the anchor gated past ±100 % on it; nothing short of doubling the guest budget would have flagged. The scaling is not a transition anyone waits out, because `cycles` has always been recorded.

| Row kind     | Narrows                                                       |
|--------------|---------------------------------------------------------------|
| `ips`        | against the current anchor, immediately                       |
| `wall_time`  | on its run's half now, and on the anchor's at the next re-bless |

#### Between-run half

The within-run half alone produced a standing false alarm. `3a-host-encode-64KiB`'s between-run move runs several times its within-run spread, and it flagged twice on a codec whose hot path had not changed.

| Archive coverage of a row  | Band                                      |
|----------------------------|-------------------------------------------|
| enough archived runs       | the wider of the two halves               |
| too few archived runs      | no estimate; the within-run half alone    |

The between-run estimate is the median *move*, not the spread of the levels. The archive spans months of accepted optimizations. A level-based estimate would read each of those steps as noise and widen the band on exactly the rows that measure cleanly.

### Measurement method

Each run stamps the method version of every suite it captured, as `methods` in the results JSON. `METHOD_VERSIONS` in `benchmark/support/roster.rb` owns the table and the reason each entry moved. A bump says a probe change rescoped what its rows measure, and it changes the gate as follows.

| After a bump            | The gate                                                     |
|-------------------------|--------------------------------------------------------------|
| Anchor comparison       | leaves that suite out entirely                               |
| Between-run estimate    | narrows to same-version runs as soon as enough exist         |
| Each gate run           | NOTEs every gated suite whose version differs from the anchor's |
| The absorbing re-bless  | puts those rows back under judgment                          |

The suite leaves the comparison because its delta is the distance between two different measurements, not a performance reading. Until same-version runs suffice, the estimate over every method stands. How far a row moves between processes survives a reordering. Dropping it at the bump would strand the noisiest rows on the floor.

### Anchor re-bless

The anchor moves only through the second command below.

```bash
rake bench:gate[current,baseline]   # compare a run against benchmark/baseline.json
rake bench:bless[run.json]          # move the anchor to run.json
```

Re-blessing is the deliberate act of accepting a new performance level. It must record the accepted shift in [Baseline changes](#baseline-changes) in the same commit. A gated case present in a run but missing from the anchor fails the gate until a re-bless records it.

### Metric per row

Each row kind gates on one metric, or on none.

| Row kind                                               | Gate metric                       |
|--------------------------------------------------------|-----------------------------------|
| sandbox-driven                                         | `wall_time`                       |
| pure host (`3a-host-decode-*` / `3a-host-encode-*`)    | median `ips`                      |
| guest-return host wrapper (`1/ips − wall_time`)        | none; characterization            |
| a `seconds` row                                        | skipped, even inside a gated suite |
| characterization suites                                | none; informational               |

The guest-return rows' host wrapper is GC/allocator-bound on the largest payloads, so it is not a gate signal. Recording a `seconds` row is how a probe declares the figure is not a release commitment. The reason is either a cold path (`1c`) or a cost paid once per Sandbox rather than once per invocation (`9a`).

### Per-row bar

The floor is a lower bound on the bar, not the bar.

| Row's own dispersion and archived move | Bar                               |
|----------------------------------------|-----------------------------------|
| both within +10 %                      | the +10 % floor                   |
| either exceeds +10 %                   | that dispersion or move instead   |

The archive half moves with every run added to it, so the bar per row is not a figure this document can hold. `rake bench:gate` names every row the archive widens on each run, clean pass included; that report is the current answer.

Read it before trusting a green pass on `#9` in particular. Its single-dispatch rows carry several percent of within-run spread on a ~50 µs guest budget. That puts their bar well above the floor, so their coverage is one against step changes rather than drift. The replay sweep's high waypoint is the row nearest the floor. It is also the one whose cost does not amortize, growing with whatever a Host App preloads. Narrowing the others is a measurement change, not a gate change.

### Re-bless decisions

Membership is deliberate, so a question sits here until a release looks at it, rather than being changed incidentally. Each would move the anchor's shape, which is the one thing a dev round must not do. The 0.21.1 bless settled the five it inherited, as the table records.

| # | Rows | Settled at 0.21.1 |
|---|------|-------------------|
| R1 | `cold_start` `1c-*` | **Accepted as ungated, in writing** |
| R2 | `catalog_handles` `5b-*` | **Accepted as ungated**, on a different ground |
| R3 | `codec`'s share of the gate | **Membership unchanged; false-alarm mechanism fixed** |
| R4 | `guest_setup` (#13) | **Stays characterization, with a trigger** |
| R5 | `9a-*` and `#9`'s single-dispatch rows | **New shape recorded**; bars accepted |

The reasoning belongs here rather than in a commit message, because the next reader of a `seconds` row will ask the same question. Nothing is open for the next bless; R4's trigger stands at two of the four archived runs it waits for.

#### Ungated rows

R1 and R2 stay ungated on different grounds.

| # | Ground                                              |
|---|-----------------------------------------------------|
| R1 | records `seconds`; worth is the cold/warm ratio    |
| R2 | content is the flatness across waypoints           |

Recording `seconds` is the whole of why R1's rows are skipped. The pair's worth is the ratio between its halves, not either level. Magnitude now also forecloses giving the warm row a gate metric: a warm `Sandbox.new` is ~3 µs against a 1 µs clock. Batching it to recover resolution would fold construction teardown into the window. That would collapse the cold/warm pair into two numbers of the same order.

R2's rows are already batched and well clear of the clock. Their content is the *flatness across waypoints*, which a per-row level comparison cannot express. The same allocation path is gated tightly by `5a-*`.

#### Codec membership

R3 kept membership and fixed the false-alarm mechanism instead. The `3c-*` rows are one encode and one decode per wire type against the payload type-mapping table. Cutting them would cut coverage, not redundancy. Case order is what produced a +13 % reading on an unchanged hot path, and that is what changed (measurement method 2):

1. `3c`
2. `3b`
3. guest rows
4. `3a`, ascending

No host row is therefore measured downstream of a payload large enough to stir the heap for it.

#### Setup-cost rows

R4 waits on a trigger, and R5 records a shape.

| # | Rows                              | Standing                                   |
|---|-----------------------------------|--------------------------------------------|
| R4 | `guest_setup` (#13)              | revisit once it carries four archived runs |
| R5 | `9a-*`                           | CPU seconds for a batch of 100             |
| R5 | `#9`'s single-dispatch rows      | bars accepted as they stand                |

R4 stays characterization, with a trigger rather than a verdict. Promoting it before it has an archive would gate it on its within-run half alone. This document already records that configuration as a standing false-alarm source.

R5's `9a` batch exists because registration is paid once per Sandbox. What `#9` buys is detection of step changes, not of drift. Narrowing it is a measurement change rather than a gate decision. It is recorded here so a green pass on those rows is not read as more than it is.

## Noise model

Two noise scales exist, and only the smaller one is visible in the reported numbers.

| Scale       | Size                                           | Visible as                 |
|-------------|------------------------------------------------|----------------------------|
| Within-run  | a few percent; worse on heavily allocating rows | `ips_sd` / `±%` per case  |
| Between-run | ±5–7 % machine transients                      | a comparison of two runs   |

The `ips_sd` / `±%` printed per case is the *within-run* sampling spread. The gate scales it to the median it guards rather than reading it whole. Comparing two runs, even minutes apart on an idle machine, additionally exposes *between-run* machine transients. The runner measures CPU time, which excludes scheduler waits but still sees frequency scaling. macOS on Apple Silicon offers no fixed-frequency governor. The `env.cpu_probe_spread_pct` field records each session's own floor.

### Interpretation rules

Three rules follow from the two scales.

1. **A uniform shift across all guest scenarios is a machine fingerprint, not a code regression**. Guest cases share one wasmtime execution cost structure, so machine state moves them together. A real regression concentrates in the touched paths.
2. **Never read `ips_sd` as the uncertainty of a cross-run comparison**. Between-run transients dominate it severalfold.
3. **Long measurement arms alias transients into fake effects**. In a worked example (2026-06-07), an A/B with 5-minute arms showed a freshly migrated Guest Binary 5–6 % slower on `mruby_eval`. It looked consistent, with tight within-arm spread. 45-second alternating arms across four guest builds then measured all of them within ±2 %. A rapid 3-pair alternation caught ±6–7 % swings between *adjacent identical processes*. The build chains had been verified equivalent (`libmruby.a` code-byte-identical), so the original signal was aliasing, not code.

### Archive limits

The gate reads the between-run scale off the archived runs (`History`, median move over the last 10). It takes the wider of that and the within-run band. Unlike the anchor, which moves only by a deliberate `bench:bless`, the archive grows whenever a run is committed. Two rules keep that from loosening the gate by accident.

| Rule                         | Limit                                           |
|------------------------------|-------------------------------------------------|
| The archive half stops       | at 30 %, three times the floor                  |
| Archive-set bars are named   | on each gate run, clean pass included           |

Past 30 % the archive says nothing useful about the row, so the bar stops rising instead of quietly switching the row off. Each named row comes with what its own run recorded alongside. A row counts only when its archive band beats both its own dispersion and the floor. Below the floor the bar is the floor either way. A pass on such a row is looser than a pass on a row the floor still governs. The NOTE is what makes the difference legible.

### Stage 2 arbitration

When `bench:gate` flags, arbitrate with stage 2.

```bash
bundle exec rake "bench:confirm[0.8.0]"          # a released version (release asset, gem fallback)
bundle exec rake "bench:confirm[path/to/a.wasm]" # an explicit Guest Binary
```

`bench:confirm` alternates the baseline and current Guest Binaries through `mruby_eval` in 3 adjacent short pairs (~5 min). It confirms a regression only when every pair agrees on direction and the mean clears ±3 %, the design that survives the transients above. Pairs spreading wider than ±20 % void the arbitration as `UNSTABLE`: the machine was not quiet, so rerun idle. Even direction-unanimity happens by chance under load.

Steady-state cost is zero; it runs only on a gate alarm. Each arm injects its Guest Binary through `KOBAKO_BENCH_WASM` and writes to a throwaway results directory. `data/kobako.wasm` and `benchmark/results/` are therefore never modified.

## Unmeasured dimensions

Every probe measures the Ruby frontend, so that is what these numbers characterize. A probe goes through `Kobako::Sandbox`, or directly against a host-side collaborator of it such as `Kobako::Codec`, `Transport::Dispatcher`, or `Catalog::Handles`. Five dimensions sit outside it, recorded here so silence is not read as coverage.

| Not covered | Why the suite cannot answer it | Standing |
|---|---|---|
| The Rust host SDK's path | no probe reaches `crates/kobako` | out until its performance is a release commitment; today only behavior is |
| The mruby VM's own call cost | no guest-local call; `2a`–`2f` read only against each other | out; detection is on the delta, and an absolute floor moves no gate |
| Guest-side setup scaling, on the gate | `#13` measures compile and binding axes as characterization only | in the suite, out of the gate; visible and unblocking |
| A String in and a String out | `2b` carries an Integer; `3c` encodes a String without dispatching one | the one gap inside the gated suite's own subject |
| Shipped artifact size | no probe reads a `.wasm`'s bytes; five release-asset variants ship unmeasured | a threshold, so it belongs to `rake gate` |

Neither a Rust host over the mruby guest nor one over a Rust guest has an arm; the parity harness pins the SDK's behavior. Promoting `#13` is a decision of its own, deliberate rather than incidental. A String in and out is the shape most Service calls take; a round-trip arm would sit beside `2b` and gate the same way. Artifact size is a threshold rather than a distribution, so it belongs to `rake gate` rather than to a benchmark.

## Known caveats

### String size cap

The guest-side codec cases stop below mruby's String cap.

| Bound                                   | Size                              |
|-----------------------------------------|-----------------------------------|
| `MRB_STR_LENGTH_MAX`, mruby's default   | 1 MiB per guest String            |
| Largest guest-side codec case           | 512 KiB                           |
| Wire payload limit                      | 16 MiB, reachable only through composite values |

### Value copies

Only an invocation value decodes without a second copy. CRuby shares a substring only when it runs to the end of its parent. At most one value per MessagePack document is therefore a view onto the wire buffer: the trailing one. An Outcome's ok body is the value alone and qualifies. A Call payload always ends with `kwargs`, so every dispatch argument is copied out of the buffer.

| Argument size on `Payload::Arguments.decode` | Added cost                                              |
|----------------------------------------------|---------------------------------------------------------|
| 1 KiB                                        | no difference (below the msgpack gem's 256-byte reference threshold) |
| 64 KiB                                       | +8.5 µs                                                 |
| 1 MiB                                        | +95 µs                                                  |

Large arguments therefore pay a copy the return path does not. The shape of the payload, not the codec, is what decides it.

### Scheduling and cache

Two conditions outside the code move these readings.

| Condition                    | Effect                                                     |
|------------------------------|------------------------------------------------------------|
| default `gvl: :hold`         | aggregate throughput GVL-bounded; multi-Thread scaling near-flat |
| `gvl: :release`              | lifts the bound for guest compute; forfeits it for dispatch-heavy work |
| cold vs hot page cache       | one-shot timings vary 5-10 %                               |

Multi-Thread scaling stays near-flat because `:hold` holds the GVL across wasm execution; the gvl suite measures what `:release` changes. One-shot timings are filesystem-cache-sensitive because the first `Sandbox.new` reads `data/kobako.wasm` from disk. Warm one-shot rows report a median across rounds for exactly this class of reason.

### Row sensitivity

Some rows read differently for reasons unrelated to a code change.

| Rows                                                            | Sensitivity                                  |
|-----------------------------------------------------------------|----------------------------------------------|
| `5a-alloc-10_000-from-empty`, `5a-alloc-100_000-from-empty`     | record no deviation                          |
| `5c`, `8d`                                                      | GC / allocator state from earlier cases      |
| `codec`                                                         | ordering is part of the measurement method   |

`ips_sd` is stored rounded to an integer, so a row near or below ~50 i/s records zero and has no within-run band. A flag on the two `5a` rows needs isolated-repeat arbitration before it is read as a regression. `5c` and `8d` are sensitive to state built up by earlier cases in the same process, so re-running a case in isolation produces different numbers. In `codec` the ordering is load-bearing enough to be part of the suite's measurement method. Changing it advances that version rather than silently re-scoping the rows.
