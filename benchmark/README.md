# Benchmarks

Kobako maintains a regression benchmark suite covering the six performance dimensions [SPEC.md](../SPEC.md) names as release regression gates (startup, Transport round-trip, codec, mruby VM, Catalog::Handles, yield round-trip) plus four characterization suites (multi-thread, per-Sandbox RSS, `#preload` + `#run` dispatch, dispatch-glue isolation).

The suite perceives drift against a fixed reference point — the committed anchor `benchmark/baseline.json` — rather than certifying a portable performance standard. Absolute numbers are meaningful only on hardware comparable to the machine that produced them; per-release runs are archived under `benchmark/results/`. A cumulative +10 % regression past the anchor on any gated benchmark blocks release until a maintainer reviews or re-blesses.

## How a case is measured

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
| `case_with_usage` | sandbox-driven `ips` cases              | adds median `wall_time` + `memory_peak` from `Sandbox#usage` (B-35) |
| `one_shot`        | cold paths (first `Sandbox.new`)        | CPU seconds — a single run (`rounds: 1`) or the median across `rounds` (warm `1c`, `5b` windows) |
| wall-clock helper | multi-thread suite                      | wall seconds — CPU time would hide scheduler overhead               |

`ips` is the **median** of per-cycle samples (a GC-inflated cycle skews a mean but not a median); the arithmetic mean rides along as `ips_mean` for the capacity reading, mirroring Google Benchmark / Criterion. For sandbox-driven cases, `case_with_usage` runs a dedicated post-measurement sampling loop (`UsageSampler`, CPU-budget-bounded) that reads `sandbox.usage` after each invocation, so `wall_time` is the median of that distribution rather than a single point sample.

## Reading the numbers

The script `benchmark/support/format_baseline.rb` is the single source of truth for unit conversions; regenerate the per-suite tables by re-running it.

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

For "N-ops-in-one-invocation" cases (e.g. `2d-1000-calls-in-one-eval`) the per-op cost is `total / N`. For "delta between waypoints" rows (e.g. `9a-1` → `9a-64`) subtract waypoints and divide by the count delta. Sandbox-driven `ips` rows also carry `wall_time` and `memory_peak`; subtract `wall_time` from `1 / ips` for the per-`#eval` host wrapper cost. Prose tables round to three significant figures, so treat sub-µs deltas as rounding noise and consult the JSON for the precise value.

## Latest baseline

The anchor is `1eee1c8`, captured **2026-07-17** — macOS arm64, Ruby 3.4.7, 16 CPUs, YJIT off. Every gated figure below is from this capture; the shift against the previous anchor — the host-codec Factory decomposition and the Handle-walk recovery — is recorded in [What changed vs previous baseline](#what-changed-vs-previous-baseline). The characterization suites (#7-#9) were re-captured in the same round.

### Lifecycle & construction

Cold-start, warm reuse, and per-request construction costs.

#### Sandbox construction and first run ([`cold_start.rb`](cold_start.rb))

Isolates Engine + Module JIT (one-time per process) from subsequent `Sandbox.new` (Engine + Module cached at process scope).

| Scenario                                                | Latency       |
|---------------------------------------------------------|---------------|
| First `Sandbox.new` in a fresh process (compiled-artifact disk cache warm) | **3.2 ms** |
| First `Sandbox.new` ever for a Guest Binary + gem version (cold disk cache) | ~500 ms, once per machine |
| Second-through-tenth `Sandbox.new` (cache warm)         | **~32 µs** each |
| Steady-state `Sandbox.new` only                         | **30.9 µs**   |
| Steady-state `Sandbox.new` + first `#eval("nil")`       | **96 µs**     |

The multi-hundred-millisecond Cranelift JIT now lands once per machine and gem version: the `.cwasm` disk cache (B-01) carries the compiled artifact across processes, so a fresh process deserializes in single-digit milliseconds instead of recompiling. The cold-cache figure is the previous anchor's fresh-process cost.

#### Reusing a Sandbox vs constructing one per request

| Pattern                                                          | Cost per request | Source                  |
|------------------------------------------------------------------|------------------|-------------------------|
| Reuse the same Sandbox (`#eval("nil")` on a warm instance)       | **78.9 µs**      | `2a-empty-call`         |
| Fresh Sandbox every request (`Kobako::Sandbox.new.eval("nil")`)  | **96 µs**        | `1b-sandbox-new+eval-nil` |
| Overhead of constructing a new Sandbox per request               | **~17 µs**       | difference              |

The per-request overhead is `Sandbox.new` itself (capture buffers, dispatch Proc, ABI probe) — under B-49 the wasm instance is created per invocation either way, so a fresh Sandbox no longer pays a per-instance boot. `wall_time` reads the guest export only; total minus `wall_time` (2a: 78.9 − 41.6 ≈ 37 µs) bundles the per-invocation instantiation with the host wrapper, so it is no longer a single-digit-µs wrapper readout.

### Wire layer (host ↔ guest)

Guest→host Transport, host→guest yield, and the codec on each side.

#### Per-request Transport latency ([`transport_roundtrip.rb`](transport_roundtrip.rb))

One guest→host Service call wrapped in one `#eval`. Each row bundles `#eval` setup (~79 µs) with the round-trip; cross-row deltas isolate the round-trip contribution. Per-call steady state is read from `2d`'s `wall_time / 1000`.

| Scenario                                                   | Latency                                | `wall_time` (guest)               |
|------------------------------------------------------------|----------------------------------------|------------------------------------|
| One Service call returning `nil`, alone in `#eval`         | **78.9 µs**                            | 41.6 µs                            |
| One Service call with one Integer arg                      | **78.0 µs**                            | 41.0 µs                            |
| One Service call with one Symbol-keyed keyword arg         | 83.0 µs                                | 44.4 µs                            |
| 1 000 sequential Service calls inside one `#eval`          | 7.22 ms total → **6.8 µs per call**    | 6.82 ms / 6.8 µs per call          |
| Handle chain — one call returns object, second targets the Handle ([B-17](../docs/behavior/dispatch.md)) | 95.7 µs | 58.6 µs |

#### Wire codec — host side ([`codec.rb`](codec.rb))

`Kobako::Codec` encode / decode directly from Ruby — no wasm boundary. Characterizes the host codec on its own; the per-wire-type table fixes one entry per SPEC.md Type Mapping row.

| Payload                                  | Encode  | Decode  |
|------------------------------------------|---------|---------|
| String, 64 B                             | 362 ns  | 568 ns  |
| String, 1 KiB                            | 496 ns  | 504 ns  |
| String, 64 KiB                           | 7.5 µs  | 2.7 µs  |
| String, 1 MiB                            | 60.3 µs | 36.1 µs |
| Array nested 1 deep (1 KiB leaf)         | 460 ns  | 592 ns  |
| Array nested 64 deep (1 KiB leaf)        | 1.0 µs  | 8.4 µs  |

| Wire type                                | Encode      | Decode      |
|------------------------------------------|-------------|-------------|
| `nil` / Boolean / Integer / Float        | 332-341 ns  | 365-386 ns  |
| Short String / binary String             | 342-347 ns  | 400-422 ns  |
| 3-element Array / 1-entry Hash           | 353-356 ns  | 640-649 ns  |
| Symbol (ext 0x00)                        | 424 ns      | 565 ns      |
| Handle (ext 0x01)                        | 478 ns      | 929 ns      |
| Exception envelope (ext 0x02)            | 996 ns      | 2.3 µs      |

#### Wire codec — guest side ([`codec.rb`](codec.rb))

Guest builds a value in mruby and returns it from `#eval`. `wall_time` isolates "guest export inside wasmtime" from the per-invocation instantiation + host wrapper (msgpack envelope + outcome decode + capture readout) that the total additionally carries (~33 µs on small payloads), so size scaling lives inside `wall_time`.

| Guest script returns                          | Latency  | `wall_time` (guest) |
|-----------------------------------------------|----------|---------------------|
| `"x" * 64` (64 B String)                      | 58.1 µs  | 25.0 µs             |
| `"x" * 1024` (1 KiB String)                   | 58.5 µs  | 25.0 µs             |
| `"x" * 65536` (64 KiB String)                 | 97.6 µs  | 45.3 µs             |
| `"x" * 524288` (512 KiB String)               | 319.4 µs | 181 µs              |
| Array nested 1 deep (1 KiB leaf)              | 59.4 µs  | 25.1 µs             |
| Array nested 64 deep (1 KiB leaf)             | 93.4 µs  | 48.1 µs             |

Note: mruby caps a single String at 1 MiB ([SPEC Invariant](../SPEC.md)); the largest guest sample here is 512 KiB. Composite values can still approach the 16 MiB wire payload limit.

#### Yield round-trip latency ([`yield_roundtrip.rb`](yield_roundtrip.rb))

Host-initiated counterpart of #2 — a Service method `yield`s into a guest-supplied block ([B-23..B-30](../docs/behavior/yield.md)). The cost lives on a different path (`YieldResponse` codec, `__kobako_yield_to_block` export, guest `BLOCK_STACK`), so a regression here is invisible to #2. Per-yield steady state is `6c` `wall_time / 1000`.

| Case                            | What it isolates                                                                          |
|---------------------------------|--------------------------------------------------------------------------------------------|
| `6a-single-yield`               | One yield (tag 0x01 ok) above the no-block #2 baseline.                                    |
| `6b-block-no-yield`             | `block_given` flag travels, Yielder built, never invoked (B-30) — re-entry-free floor.     |
| `6c-1000-yields-in-one-call`    | 1 000 yields in one dispatch (J-06 shape) — load-bearing for `each`-style Services.        |
| `6d-yield-break`                | Block runs `break` on first yield (tag 0x02), unwinding via catch/throw (B-25).            |

| Case                            | Latency                            | `wall_time` (guest)        |
|---------------------------------|------------------------------------|----------------------------|
| `6a-single-yield`               | 82.2 µs                            | 43.4 µs                    |
| `6b-block-no-yield`             | 79.4 µs                            | 40.7 µs                    |
| `6c-1000-yields-in-one-call`    | 3.84 ms → **3.8 µs per yield**     | 3.68 ms / 3.7 µs per yield |
| `6d-yield-break`                | 220.7 µs                           | 182.5 µs                   |

`6c` gates on `wall_time` so the 1 000-element host-side decode is excluded from the gated metric.

### mruby VM & Handle table

Pure interpreter work and the host-side ID→object table.

#### mruby VM, no Transport ([`mruby_eval.rb`](mruby_eval.rb))

Self-contained mruby computations whose only host cost is the constant `Sandbox#eval` overhead. Regression signal for `build_config/wasi.rb` flag changes and the IO write path.

| Script                                                        | Latency                                          | `wall_time` (guest)       |
|---------------------------------------------------------------|--------------------------------------------------|---------------------------|
| 100 000-iteration integer XOR loop                            | **42.40 ms**                                     | 42.38 ms                  |
| 1 000 single-character String appends                         | 504 µs                                           | 467 µs                    |
| 100 cycles of `raise` / `rescue`                              | 216 µs → 2.2 µs per cycle                        | 179 µs                    |
| 1 000 `puts` of 64 B (below 1 MiB stdout cap)                 | 2.75 ms → 2.7 µs per write                       | 2.62 ms                   |
| 2 048 `puts` of ~1 KiB against the 1 MiB stdout cap           | 5.86 ms (first ~1 024 land, rest silently dropped) | 5.51 ms                 |

`4e` holds at the 64 KiB per-invocation memory floor and `4f` reaches only 192 KiB — both far below the bytes written — confirming the IO write path is wasi-libc-bound, not guest-linear-memory-bound; `stdout_truncated?` flips to `true` after `4f`.

#### Handle table scaling ([`catalog_handles.rb`](catalog_handles.rb))

`Catalog::Handles` is the host-side ID→object mapping, reset at the start of every invocation. The 1 K → 1 M waypoint rows verify the underlying dictionary stays O(1) as it grows; the `5c` row deliberately measures `#eval` cost under sustained heap pressure that `1b` cannot detect.

| Scenario                                                            | Latency                  |
|---------------------------------------------------------------------|--------------------------|
| Allocate one Handle in an empty table                               | 645 ns                   |
| Allocate 100 Handles from empty                                     | 50.6 µs total            |
| Allocate 10 000 Handles from empty                                  | 5.01 ms total            |
| Allocate 100 000 Handles from empty                                 | 57.5 ms total            |
| 1 000 allocs against a 1 K-entry table                              | 0.433 ms                 |
| 1 000 allocs against a 10 K-entry table                             | 0.444 ms                 |
| 1 000 allocs against a 100 K-entry table                            | 0.452 ms                 |
| 1 000 allocs against a 1 M-entry table                              | 0.585 ms                 |
| Warm `#eval("nil")` under sustained heap pressure (1 M-entry table) | 63.0 µs (`wall_time` = 22.8 µs) |

Per-alloc cost holds 433-585 ns across four orders of magnitude — the gentle climb is allocator state, not lookup curve. ([B-21](../docs/behavior/dispatch.md) caps the counter at `0x7fff_ffff`; the cap guard is constant-time and not iterated here.)

### Setup-once dispatch (characterization only)

#### `#preload` + `#run` dispatch ([`preload_dispatch.rb`](preload_dispatch.rb))

`#preload(code:)` registers snippets that replay against the canonical boot state on every invocation; `#run(:Target)` dispatches into a preloaded entrypoint. The rows isolate each verb's contribution via waypoint deltas.

```
   9a sweep:  Sandbox.new + 1 / 8 / 64 #preload     ─▶ delta / Δsnippets ≈ 2.3 µs per snippet preload
   9e sweep:  warm #run with 0 / 8 / 64 snippets    ─▶ delta / Δsnippets ≈ 8.0 µs per snippet replay
```

| Scenario                                                            | Latency  | `wall_time` (guest) |
|---------------------------------------------------------------------|----------|---------------------|
| `Sandbox.new` + 1 `#preload(code:)`                                 | 36 µs    | —                   |
| `Sandbox.new` + 8 `#preload(code:)`                                 | 47 µs    | —                   |
| `Sandbox.new` + 64 `#preload(code:)`                                | 180 µs   | —                   |
| Warm `#run(:Noop)` (1 entrypoint preloaded)                         | 104 µs   | 57.9 µs             |
| Warm `#run(:Echo, 42)` (positional arg)                             | 107 µs   | 61.2 µs             |
| Warm `#run(:Greet, name: :alice)` (Symbol-keyed kwargs)             | 113 µs   | 57.7 µs             |
| Warm `#run(:Wrap, StringIO)` (B-34 host→guest auto-wrap)            | 102 µs   | 48.7 µs             |
| Warm `#run(:Noop)` with 0 helper snippets preloaded                 | 97 µs    | 45.5 µs             |
| Warm `#run(:Noop)` with 8 helper snippets preloaded                 | 153 µs   | 93.8 µs             |
| Warm `#run(:Noop)` with 64 helper snippets preloaded                | 610 µs   | 551 µs              |

`9a` rows carry no `wall_time` — the timer wraps `Sandbox.new + #preload` and neither calls the guest export. A `deep_wrap` / `Catalog::Handles#alloc` super-linear regression would show as `9f` rising above `9c`.

### Operational characterization (not gated)

#### Multi-Thread behavior ([`concurrent/threads.rb`](concurrent/threads.rb))

`ext/` does not call `rb_thread_call_without_gvl` during wasm execution, so wasm-side work is GVL-serialized; Ruby-side `#eval` setup can still overlap. Wall-clock timing because that is where scheduler effects manifest.

| Scenario                                                           | Result          |
|--------------------------------------------------------------------|-----------------|
| 1 Thread, owning one Sandbox                                       | 14.2k `#eval`/s |
| 2 Threads, each owning one Sandbox                                 | 13.7k `#eval`/s |
| 4 Threads, each owning one Sandbox                                 | 15.3k `#eval`/s |
| 8 Threads, each owning one Sandbox                                 | 14.2k `#eval`/s |
| Per-Sandbox `Sandbox.new` cost, single-Threaded                    | 0.206 ms        |
| Per-Sandbox `Sandbox.new` cost, 8 Threads in parallel              | 0.089 ms each (0.714 ms total / 8) |
| `#eval("nil")` baseline                                            | 0.062 ms        |
| `#eval("nil")` while another Thread is in a long `#eval`           | 0.175 ms (2.8× baseline) |

A long-running script does not block other Threads' short `#eval` calls by hundreds of ms — host-side synchronization yields the GVL and the contending Thread interleaves. Run-to-run ratio swings 1.5-3× with scheduler quirks; the order of magnitude is the regression signal.

#### Dispatch-glue isolation ([`dispatch_glue.rb`](dispatch_glue.rb))

The predictive half of the GVL-impact toolkit; the Multi-Thread suite above (#7) is the confirmation half. It calls `Kobako::Transport::Dispatcher.dispatch` directly with pre-encoded Request bytes — no wasm, boundary, or guest codec in the window — to isolate `G`, the GVL-held host glue of one guest→host dispatch (decode → resolve → invoke → encode). A No-GVL design would parallelize everything *except* this glue, so the multi-core speedup ceiling for an invocation doing `k` dispatches in wall-time `T` is Amdahl-bounded by `d = k·G / T`. The Services are pure-CPU on purpose: a Service doing real I/O releases the GVL during the syscall, so its wait already overlaps today and must not count toward `G`. Captured 2026-07-17 on `1eee1c8`.

| Case                          | `G` per dispatch | What it isolates                                     |
|-------------------------------|------------------|------------------------------------------------------|
| `10a-empty-call`              | 3.7 µs           | Floor: decode 5-field + path lookup + invoke + encode nil |
| `10b-primitive-arg`           | 3.9 µs           | + one Integer arg                                    |
| `10c-kwargs`                  | 5.0 µs           | + Symbol-keyed kwargs (ext 0x00)                     |
| `10d-small-return-16`         | 4.5 µs           | Service returns a 16-element Array                   |
| `10e-large-return-256`        | 11.7 µs          | 256-element Array — `G` grows with returned payload  |

Compose with the full roundtrip (`transport_roundtrip` `2d` ≈ 6.8 µs/call) for the per-dispatch floor of `d`: glue 3.7 µs of a 6.8 µs roundtrip ⇒ `d ≈ 0.54`, since the remaining ~46 % (guest codec + boundary) parallelizes. So even a pure-dispatch workload has a ~1.85× multi-core ceiling, rising toward `N×` as compute per invocation grows. `G` is the gem-controlled glue floor only — a Service's own Ruby CPU is the Host App's to measure, so the gem publishes `G` and the method, never a single `d`.

#### Memory cost ([`memory.rb`](memory.rb))

External RSS sampling (`ps -o rss=`) only — never reaches inside the Sandbox's mruby heap or Wasm linear memory. The granularity that capacity planning needs without violating SPEC's Non-Goal on per-invocation instrumentation.

| Scenario                                                              | RSS                                                                            | B-35 `memory_peak`           |
|-----------------------------------------------------------------------|--------------------------------------------------------------------------------|------------------------------|
| Process RSS at boot (no Sandbox)                                      | 26.5 MB                                                                        | —                            |
| RSS after the first `Sandbox.new` + `#eval("nil")`                    | 32.1 MB (**+5.6 MB** — Engine init + `.cwasm` deserialize, one-time)           | —                            |
| RSS after 10 Sandboxes total                                          | 32.1 MB (<1 KB per additional Sandbox)                                         | —                            |
| RSS after 100 Sandboxes total                                         | 32.1 MB (<1 KB per additional Sandbox)                                         | —                            |
| RSS after 1 000 Sandboxes total                                       | 33.5 MB (~**1.5 KB per additional Sandbox**)                                   | —                            |
| RSS drift after 10 000 consecutive `#eval("nil")` on one Sandbox      | +3.6 MB, still gently climbing at 10 000                                       | **64 KiB** per invocation (one `memory.grow` above the baked image) |
| Peak RSS while holding a 512 KiB return value                         | +1.0 MB above baseline                                                         | **2.1 MiB** guest `memory.grow` |
| Retained RSS after GC of the same value                               | +1.0 MB (allocator does not eagerly return pages to the OS)                    | —                            |
| Peak RSS while holding a 1 MiB capped stdout buffer                   | +3.2 MB above baseline (allocator-state-dependent)                             | **192 KiB** (stdout flows via WASI pipe, not linear memory) |
| Retained RSS after GC of the same capture                             | +3.2 MB                                                                        | —                            |

Budget ~32 MB up front per worker process; an idle Sandbox holds no wasm instance under B-49, so additional Sandboxes cost KB, not MB — **1 000 tenants ≈ 33 MB** in one Ruby process. Per-invocation linear memory lives and dies with the invocation's instance; RSS figures swing with host load and allocator state, so treat them as ranges.

#### Regexp engine ([`regexp.rb`](regexp.rb))

Regexp is an opt-in capability gem, excluded from the gated default binary, so this suite runs against the `+regexp-unicode` variant and never blocks release. Each row is a 1 000-iteration loop over a 25-byte subject, captured 2026-06-10 on `870fdc4`.

| Scenario                                                   | Throughput | Per op           |
|------------------------------------------------------------|------------|------------------|
| `=~` literal in a loop (recompiles each iteration)         | 166 i/s    | 6.0 µs / match   |
| `=~` hoisted (compiled once)                               | 196 i/s    | 5.1 µs / match   |
| `match?` hoisted                                           | 824 i/s    | 1.2 µs / match   |
| `Regexp.compile` ×1 000, no match                          | 767 i/s    | 1.3 µs / compile |
| empty 1 000-loop (overhead only)                           | 2.44k i/s  | 0.4 µs           |
| capturing `match`                                          | 172 i/s    | 5.8 µs / match   |
| `scan` every word of a sentence                            | 233 i/s    | 4.3 µs / scan    |
| `gsub` upcasing every word (block)                         | 23 i/s     | 43 µs / gsub     |
| `split` on a delimiter pattern                             | 334 i/s    | 3.0 µs / split   |

`=~` costs ~4× `match?` because it eagerly builds the `MatchData` and refreshes the match globals every call, which `match?` skips — reach for `match?` for boolean tests. The literal-in-loop vs hoisted gap stays small because the RX-08 per-invocation compile cache absorbs mruby's recompile-per-literal.

## What changed vs previous baseline

Diff against the immediately previous baseline only; pre-history lives in `benchmark/results/<date>-<sha>.json` and release-tagged `benchmark/<semver>` annotated tags.

**Previous baseline:** `2168a19`, 2026-07-08 (the round that stayed current with wasmtime 45→46 and absorbed two accepted correctness costs — nested-Handle dispatch resolution and beni 0.7 protected dispatch). **This baseline:** `1eee1c8`, 2026-07-17 — the polish round that landed the parked host-side optimizations against an unchanged guest binary: the codec Factory decomposition and the Handle-walk skip.

### Roster / schema

Unchanged — every gated label matches `2168a19`. The dispatch-glue suite (#10) is now captured into the anchor file, having previously been documented from a standalone 2026-06-17 run.

### Metric deltas vs `2168a19`

Real improvements, each clearing the +10 % floor and its noise band:

- **Host codec +30–63 % ips across encode/decode**, largest on the smallest payloads. The Factory decomposition (`af496e5` → `094af4e` → `c8f7346`) dissolved the per-call `Forwardable` delegation wrapper into a codec-private `State`, so every pack/unpack drops a fixed indirection. The gain tapers with payload size — ~2× on primitives, within noise by 16 MiB — because the removed cost is per-call, not per-byte, so it is invisible in the end-to-end paths that wasm execution dominates.
- **`6d-yield-break` −27 % (290 → 220 µs)** — the parked Handle-walk-skip optimization landed (`cd63514` / `d2c4947` / `bca463b`). The codec records on decode whether an ext 0x01 Handle crossed the wire and skips `Codec::HandleWalk.deep_restore` entirely when none did — a Handle-free argument/result/yield tree resolves to itself. This recovers the regression `6d` absorbed at `2168a19` and drops below its pre-regression level; behaviour is unchanged because the skipped walk was already an identity pass.
- Minor: `4e`/`4f` stdout −9–10 % and `4a` −7 % on `wall_time` cleared their noise bands; guest compute is otherwise flat against the unchanged binary.

No accepted regressions this round. Two readings that look like regressions are not: the end-to-end totals (`2a`/`2b`/`2c`/`2e`, `6a`/`6b`, `1a`) read ~5–8 % higher than `2168a19` but every one stayed inside its noise band — a capture transient, with the gated `wall_time` rows and `1b` (96 µs) flat. The #7 concurrent suite was captured under elevated machine load, so its wall-clock throughput reads ~14–15k vs the quiet-machine ~17k `#eval`/s; the gated `1b` / `5c` eval-nil cost is unchanged, confirming the shortfall is load, not code. The cumulative budget resets to these blessed numbers.

## Running

```bash
bundle exec rake bench                   # six gated benchmarks (CI-friendly, payloads ≤ 1 MiB)
bundle exec rake bench:full              # adds the 16 MiB codec payload sweep
bundle exec rake bench:concurrent        # multi-Thread characterization (#7)
bundle exec rake bench:memory            # per-Sandbox RSS characterization (#8)
bundle exec rake bench:preload_dispatch  # #preload + #run characterization (#9)
bundle exec rake bench:dispatch_glue     # dispatch-glue isolation characterization (#10)
```

Each rake task shells out to `bundle exec ruby benchmark/<file>.rb`; invoke a single script directly for fast iteration. `bundle exec rake bench` runs in 5-8 min on a current-gen laptop (codec dominates with 46 cases × 3 s warmup + 3 s measurement); each characterization task adds 30 s to 1 min.

YJIT is not turned on by the suite. Use `RUBY_YJIT_ENABLE=1 bundle exec rake bench` or `--yjit` to capture a YJIT baseline — the resulting JSON records `yjit_enabled: true` so it is unambiguously distinct.

## Result files

Every run writes (or merges into) `benchmark/results/<date>-<short-sha>.json`:

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
                               "wall_time": 0.0001273, "wall_time_sd": 0.0000191, "memory_peak": 0 } ],
    ...
  }
}
```

| Field                                                | Meaning                                                                                       |
|------------------------------------------------------|-----------------------------------------------------------------------------------------------|
| `ips`                                                | Median of per-cycle iterations / CPU second; higher is better.                                |
| `ips_mean`                                           | Arithmetic mean of the per-cycle `ips` samples.                                               |
| `ips_sd`                                             | Standard deviation of the per-cycle `ips` samples.                                            |
| `iterations` / `cycles`                              | Total iterations measured and number of samples collected within the time budget.             |
| `seconds` / `rounds`                                 | `one_shot` CPU seconds (the median across `rounds` when > 1); wall seconds on the multi-thread suite. |
| `env.load_avg` / `env.power_source` / `env.cpu_probe_spread_pct` | Machine state at capture: 1-minute load, AC vs battery, and the spread between two back-to-back runs of a fixed pure-CPU probe — the session's own noise floor. |
| `wall_time` / `wall_time_sd` / `memory_peak`         | Sandbox-driven rows only (B-35). Median of `Sandbox#usage` samples; `memory_peak` is `memory.grow` delta past the per-invocation baseline. Annotate-only rows (`1b`) carry one sample with no dispersion. |

Release baselines are additionally marked with `benchmark/<semver>` annotated git tags.

## Release gate

`rake bench:gate[current,baseline]` compares a run against the committed anchor `benchmark/baseline.json` and exits non-zero on either a gated case regressed past the anchor or a gated case the anchor does not yet cover. The comparison logic lives in `benchmark/support/gate.rb` (behind the `Kobako::Bench` facade); its unit tests ride the test suite (`rake test:bench`).

A case is flagged only when its regression past the anchor clears **both** a +10 % floor (cumulative against the anchor, not the previous run) **and** a noise band of `2 × √(cv_current² + cv_baseline²)`. The noise band can only widen the bar on high-variance rows, never narrow it below the floor.

The anchor moves only via `rake bench:bless[run.json]` — re-blessing is the deliberate act of accepting a new performance level and must record the accepted shift in [What changed vs previous baseline](#what-changed-vs-previous-baseline) in the same commit. A gated case present in a run but missing from the anchor fails the gate until a re-bless records it.

**Metric per row:** sandbox-driven rows gate on `wall_time`; pure host rows (`3a-host-decode-*` / `3a-host-encode-*`) gate on median `ips`; the guest-return rows' host wrapper (`1/ips − wall_time`) is GC/allocator-bound on the largest payloads and is characterization, not a gate signal. One-shot / cold-path rows carry no dispersion and are skipped. The three characterization suites (#7 / #8 / #9) are informational and not part of the gate.

The gate is **stage 1** — a smoke detector against the anchor. A flag is a reason to arbitrate, not yet a verdict; see the next section for stage 2.

## Noise model and interpretation

Two noise scales exist, and only the smaller one is visible in the reported numbers. The `ips_sd` / `±%` printed per case is the *within-run* sampling spread (±0.5–2 %). Comparing two runs — even minutes apart on an idle machine — additionally exposes *between-run* machine transients of ±5–7 %: the runner measures CPU time, which excludes scheduler waits but still sees frequency scaling, and macOS on Apple Silicon offers no fixed-frequency governor. The `env.cpu_probe_spread_pct` field records each session's own floor.

Interpretation rules:

- **A uniform shift across all guest scenarios is a machine fingerprint, not a code regression.** Guest cases share one wasmtime execution cost structure, so machine state moves them together; a real regression concentrates in the touched paths.
- **Never read `ips_sd` as the uncertainty of a cross-run comparison** — between-run transients dominate it severalfold.
- **Long measurement arms alias transients into fake effects.** Worked example (2026-06-07): an A/B with 5-minute arms showed a freshly migrated Guest Binary a consistent-looking 5–6 % slower on `mruby_eval` with tight within-arm spread; 45-second alternating arms across four guest builds then measured all of them within ±2 %, and a rapid 3-pair alternation caught ±6–7 % swings between *adjacent identical processes*. The build chains had been verified equivalent (`libmruby.a` code-byte-identical), so the original signal was aliasing, not code.

When `bench:gate` flags, arbitrate with stage 2:

```bash
bundle exec rake "bench:confirm[0.8.0]"          # a released version (release asset, gem fallback)
bundle exec rake "bench:confirm[path/to/a.wasm]" # an explicit Guest Binary
```

`bench:confirm` alternates the baseline and current Guest Binaries through `mruby_eval` in 3 adjacent short pairs (~5 min) and confirms a regression only when every pair agrees on direction **and** the mean clears ±3 % — the design that survives the transients above. Pairs spreading wider than ±20 % void the arbitration as `UNSTABLE` (the machine was not quiet — rerun idle; even direction-unanimity happens by chance under load). Steady-state cost is zero; it runs only on a gate alarm. Each arm injects its Guest Binary through `KOBAKO_BENCH_WASM` and writes to a throwaway results directory, so `data/kobako.wasm` and `benchmark/results/` are never modified.

## Known caveats

- **Guest String size cap at 1 MiB.** `MRB_STR_LENGTH_MAX` is mruby's default; the guest-side codec cases stop at 512 KiB. The 16 MiB wire payload limit is reachable only through composite values.
- **Aggregate throughput is GVL-bounded.** Multi-Thread scaling stays near-flat because `ext/` does not release the GVL during wasmtime execution.
- **One-shot timings are filesystem-cache-sensitive.** The first `Sandbox.new` reads `data/kobako.wasm` from disk; cold vs hot page cache can vary 5-10 %. Warm one-shot rows report a median across rounds for exactly this class of reason.
- **Per-suite ordering matters.** `5c` and `8d` are sensitive to GC / allocator state built up by earlier cases in the same process; re-running a case in isolation produces different numbers.
