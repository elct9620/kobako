# Benchmarks

Kobako maintains a regression benchmark suite covering the six performance dimensions [SPEC.md](../SPEC.md) names as release regression gates (startup, Transport round-trip, codec, mruby VM, Catalog::Handles, yield round-trip) plus three characterization suites (multi-thread, per-Sandbox RSS, `#preload` + `#run` dispatch).

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
| `one_shot`        | cold paths (first `Sandbox.new`)        | CPU seconds for a single run, no dispersion                         |
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

Captured on **2026-05-27** at commit `711665d` (the 0.5.0 release line) — macOS arm64, Ruby 3.4.7, 16 CPUs, YJIT off.

### Lifecycle & construction

Cold-start, warm reuse, and per-request construction costs.

#### Sandbox construction and first run ([`cold_start.rb`](cold_start.rb))

Isolates Engine + Module JIT (one-time per process) from subsequent `Sandbox.new` (Engine + Module cached at process scope).

| Scenario                                                | Latency       |
|---------------------------------------------------------|---------------|
| First `Sandbox.new` in a fresh process                  | **629 ms**    |
| Second-through-tenth `Sandbox.new` (cache warm)         | **~0.11 ms** each |
| Steady-state `Sandbox.new` only                         | **125 µs**    |
| Steady-state `Sandbox.new` + first `#eval("nil")`       | **272 µs**    |

Pre-warm by constructing one Sandbox at boot — after that the per-request cost is microseconds, not the multi-hundred-millisecond JIT.

#### Reusing a Sandbox vs constructing one per request

| Pattern                                                          | Cost per request | Source                  |
|------------------------------------------------------------------|------------------|-------------------------|
| Reuse the same Sandbox (`#eval("nil")` on a warm instance)       | **136 µs**       | `2a-empty-call`         |
| Fresh Sandbox every request (`Kobako::Sandbox.new.eval("nil")`)  | **272 µs**       | `1b-sandbox-new+eval-nil` |
| Overhead of constructing a new Sandbox per request               | **~136 µs**      | difference              |

The per-request overhead is `Sandbox.new` itself (Wasm instance, capture buffers, dispatch Proc) — not a repeated cold-start cost, which is amortized to the first Sandbox in the process. For the host-wrapper / guest-export split read `2a`'s **median** `wall_time` (127 µs against 136 µs total → single-digit µs host wrapper); `1b`'s single post-loop sample is fresh-Sandbox-first-`#eval` and is not reliable as a split basis.

### Wire layer (host ↔ guest)

Guest→host Transport, host→guest yield, and the codec on each side.

#### Per-request Transport latency ([`transport_roundtrip.rb`](transport_roundtrip.rb))

One guest→host Service call wrapped in one `#eval`. Each row bundles `#eval` setup (~130 µs) with the round-trip; cross-row deltas isolate the round-trip contribution. Per-call steady state is read from `2d`'s `wall_time / 1000`.

| Scenario                                                   | Latency                                | `wall_time` (guest)               |
|------------------------------------------------------------|----------------------------------------|------------------------------------|
| One Service call returning `nil`, alone in `#eval`         | **136 µs**                             | 127 µs                             |
| One Service call with one Integer arg                      | **138 µs**                             | 128 µs                             |
| One Service call with one Symbol-keyed keyword arg         | 139 µs                                 | 131 µs                             |
| 1 000 sequential Service calls inside one `#eval`          | 6.74 ms total → **6.7 µs per call**    | 6.73 ms / 6.7 µs per call          |
| Handle chain — one call returns object, second targets the Handle ([B-17](../docs/behavior.md)) | 152 µs | 142 µs |

#### Wire codec — host side ([`codec.rb`](codec.rb))

`Kobako::Codec` encode / decode directly from Ruby — no wasm boundary. Characterizes the host codec on its own; the per-wire-type table fixes one entry per SPEC.md Type Mapping row.

| Payload                                  | Encode  | Decode  |
|------------------------------------------|---------|---------|
| String, 64 B                             | 541 ns  | 581 ns  |
| String, 1 KiB                            | 644 ns  | 668 ns  |
| String, 64 KiB                           | 7.6 µs  | 2.9 µs  |
| String, 1 MiB                            | 59.3 µs | 36.0 µs |
| Array nested 1 deep (1 KiB leaf)         | 676 ns  | 800 ns  |
| Array nested 64 deep (1 KiB leaf)        | 1.1 µs  | 8.8 µs  |

| Wire type                                | Encode      | Decode      |
|------------------------------------------|-------------|-------------|
| `nil` / Boolean / Integer / Float        | 524-529 ns  | 530-556 ns  |
| Short String / binary String             | 537-541 ns  | 563-590 ns  |
| 3-element Array / 1-entry Hash           | 554-558 ns  | 815-917 ns  |
| Symbol (ext 0x00)                        | 619 ns      | 730 ns      |
| Handle (ext 0x01)                        | 657 ns      | 1.0 µs      |
| Exception envelope (ext 0x02)            | 1.3 µs      | 2.9 µs      |

#### Wire codec — guest side ([`codec.rb`](codec.rb))

Guest builds a value in mruby and returns it from `#eval`. `wall_time` isolates "guest export inside wasmtime" from the per-`#eval` host wrapper (msgpack envelope + outcome decode + capture readout) — the host wrapper holds steady at ~12-15 µs, so size scaling lives inside `wall_time`.

| Guest script returns                          | Latency  | `wall_time` (guest) |
|-----------------------------------------------|----------|---------------------|
| `"x" * 64` (64 B String)                      | 123.5 µs | 116 µs              |
| `"x" * 1024` (1 KiB String)                   | 124.4 µs | 116 µs              |
| `"x" * 65536` (64 KiB String)                 | 158.1 µs | 143 µs              |
| `"x" * 524288` (512 KiB String)               | 449.4 µs | 333 µs              |
| Array nested 1 deep (1 KiB leaf)              | 125.3 µs | 117 µs              |
| Array nested 64 deep (1 KiB leaf)             | 164.4 µs | 147 µs              |

Note: mruby caps a single String at 1 MiB ([SPEC Invariant](../SPEC.md)); the largest guest sample here is 512 KiB. Composite values can still approach the 16 MiB wire payload limit.

#### Yield round-trip latency ([`yield_roundtrip.rb`](yield_roundtrip.rb))

Host-initiated counterpart of #2 — a Service method `yield`s into a guest-supplied block ([B-23..B-30](../docs/behavior.md)). The cost lives on a different path (`YieldResponse` codec, `__kobako_yield_to_block` export, guest `BLOCK_STACK`), so a regression here is invisible to #2. Per-yield steady state is `6c` `wall_time / 1000`.

| Case                            | What it isolates                                                                          |
|---------------------------------|--------------------------------------------------------------------------------------------|
| `6a-single-yield`               | One yield (tag 0x01 ok) above the no-block #2 baseline.                                    |
| `6b-block-no-yield`             | `block_given` flag travels, Yielder built, never invoked (B-30) — re-entry-free floor.     |
| `6c-1000-yields-in-one-call`    | 1 000 yields in one dispatch (J-06 shape) — load-bearing for `each`-style Services.        |
| `6d-yield-break`                | Block runs `break` on first yield (tag 0x02), unwinding via catch/throw (B-25).            |

| Case                            | Latency                            | `wall_time` (guest)        |
|---------------------------------|------------------------------------|----------------------------|
| `6a-single-yield`               | 143 µs                             | 133 µs                     |
| `6b-block-no-yield`             | 138 µs                             | 129 µs                     |
| `6c-1000-yields-in-one-call`    | 3.86 ms → **3.9 µs per yield**     | 3.89 ms / 3.9 µs per yield |
| `6d-yield-break`                | 270 µs                             | 258 µs                     |

`6c` gates on `wall_time` so the 1 000-element host-side decode is excluded from the gated metric.

### mruby VM & Handle table

Pure interpreter work and the host-side ID→object table.

#### mruby VM, no Transport ([`mruby_eval.rb`](mruby_eval.rb))

Self-contained mruby computations whose only host cost is the constant `Sandbox#eval` overhead. Regression signal for `build_config/wasi.rb` flag changes and the IO write path.

| Script                                                        | Latency                                          | `wall_time` (guest)       |
|---------------------------------------------------------------|--------------------------------------------------|---------------------------|
| 100 000-iteration integer XOR loop                            | **42.83 ms**                                     | 42.71 ms                  |
| 1 000 single-character String appends                         | 585 µs                                           | 575 µs                    |
| 100 cycles of `raise` / `rescue`                              | 301 µs → 3.0 µs per cycle                        | 303 µs                    |
| 1 000 Onigmo `Regexp =~` matches                              | 3.01 ms → 3.0 µs per match                       | 3.01 ms                   |
| 1 000 `puts` of 64 B (below 1 MiB stdout cap)                 | 4.26 ms → 4.3 µs per write                       | 4.17 ms                   |
| 2 048 `puts` of ~1 KiB against the 1 MiB stdout cap           | 9.28 ms (first ~1 024 land, rest silently dropped) | 8.54 ms                 |

`memory_peak = 0` on `4e` / `4f` confirms the IO write path is wasi-libc-bound, not guest-linear-memory-bound; `stdout_truncated?` flips to `true` after `4f`.

#### Handle table scaling ([`catalog_handles.rb`](catalog_handles.rb))

`Catalog::Handles` is the host-side ID→object mapping, reset at the start of every invocation. The 1 K → 1 M waypoint rows verify the underlying dictionary stays O(1) as it grows; the `5c` row deliberately measures `#eval` cost under sustained heap pressure that `1b` cannot detect.

| Scenario                                                            | Latency                  |
|---------------------------------------------------------------------|--------------------------|
| Allocate one Handle in an empty table                               | 541 ns                   |
| Allocate 100 Handles from empty                                     | 42.1 µs total            |
| Allocate 10 000 Handles from empty                                  | 4.08 ms total            |
| Allocate 100 000 Handles from empty                                 | 44.24 ms total           |
| 1 000 allocs against a 1 K-entry table                              | 0.349 ms                 |
| 1 000 allocs against a 10 K-entry table                             | 0.340 ms                 |
| 1 000 allocs against a 100 K-entry table                            | 0.353 ms                 |
| 1 000 allocs against a 1 M-entry table                              | 0.487 ms                 |
| Warm `#eval("nil")` under sustained heap pressure (1 M-entry table) | 122 µs (`wall_time` = 115 µs) |

Per-alloc cost holds 349-487 ns across four orders of magnitude — the gentle climb is allocator state, not lookup curve. ([B-21](../docs/behavior.md) caps the counter at `0x7fff_ffff`; the cap guard is constant-time and not iterated here.)

### Setup-once dispatch (characterization only)

#### `#preload` + `#run` dispatch ([`preload_dispatch.rb`](preload_dispatch.rb))

`#preload(code:)` registers snippets that replay against the fresh `mrb_state` on every invocation; `#run(:Target)` dispatches into a preloaded entrypoint. The rows isolate each verb's contribution via waypoint deltas.

```
   9a sweep:  Sandbox.new + 1 / 8 / 64 #preload     ─▶ delta / Δsnippets ≈ 2.1 µs per snippet preload
   9e sweep:  warm #run with 0 / 8 / 64 snippets    ─▶ delta / Δsnippets ≈ 7-9 µs per snippet replay
```

| Scenario                                                            | Latency  | `wall_time` (guest) |
|---------------------------------------------------------------------|----------|---------------------|
| `Sandbox.new` + 1 `#preload(code:)`                                 | 126 µs   | —                   |
| `Sandbox.new` + 8 `#preload(code:)`                                 | 133 µs   | —                   |
| `Sandbox.new` + 64 `#preload(code:)`                                | 259 µs   | —                   |
| Warm `#run(:Noop)` (1 entrypoint preloaded)                         | 163 µs   | 151 µs              |
| Warm `#run(:Echo, 42)` (positional arg)                             | 163 µs   | 151 µs              |
| Warm `#run(:Greet, name: :alice)` (Symbol-keyed kwargs)             | 166 µs   | 153 µs              |
| Warm `#run(:Wrap, StringIO)` (B-34 host→guest auto-wrap)            | 157 µs   | 141 µs              |
| Warm `#run(:Noop)` with 0 helper snippets preloaded                 | 151 µs   | 138 µs              |
| Warm `#run(:Noop)` with 8 helper snippets preloaded                 | 208 µs   | 193 µs              |
| Warm `#run(:Noop)` with 64 helper snippets preloaded                | 735 µs   | 730 µs              |

`9a` rows carry no `wall_time` — the timer wraps `Sandbox.new + #preload` and neither calls the guest export. A `deep_wrap` / `Catalog::Handles#alloc` super-linear regression would show as `9f` rising above `9c`.

### Operational characterization (not gated)

#### Multi-Thread behavior ([`concurrent/threads.rb`](concurrent/threads.rb))

`ext/` does not call `rb_thread_call_without_gvl` during wasm execution, so wasm-side work is GVL-serialized; Ruby-side `#eval` setup can still overlap. Wall-clock timing because that is where scheduler effects manifest.

| Scenario                                                           | Result          |
|--------------------------------------------------------------------|-----------------|
| 1 Thread, owning one Sandbox                                       | 7.8k `#eval`/s  |
| 2 Threads, each owning one Sandbox                                 | 6.6k `#eval`/s  |
| 4 Threads, each owning one Sandbox                                 | 7.4k `#eval`/s  |
| 8 Threads, each owning one Sandbox                                 | 7.5k `#eval`/s  |
| Per-Sandbox `Sandbox.new` cost, single-Threaded                    | 0.175 ms        |
| Per-Sandbox `Sandbox.new` cost, 8 Threads in parallel              | 0.163 ms each (1.306 ms total / 8) |
| `#eval("nil")` baseline                                            | 0.134 ms        |
| `#eval("nil")` while another Thread is in a long `#eval`           | 0.199 ms (1.5× baseline) |

A long-running script does not block other Threads' short `#eval` calls by hundreds of ms — host-side synchronization yields the GVL and the contending Thread interleaves. Run-to-run ratio swings 1.5-3× with scheduler quirks; the order of magnitude is the regression signal.

#### Memory cost ([`memory.rb`](memory.rb))

External RSS sampling (`ps -o rss=`) only — never reaches inside the Sandbox's mruby heap or Wasm linear memory. The granularity that capacity planning needs without violating SPEC's Non-Goal on per-invocation instrumentation.

| Scenario                                                              | RSS                                                                            | B-35 `memory_peak`           |
|-----------------------------------------------------------------------|--------------------------------------------------------------------------------|------------------------------|
| Process RSS at boot (no Sandbox)                                      | 25.3 MB                                                                        | —                            |
| RSS after the first `Sandbox.new` + `#eval("nil")`                    | 137.9 MB (**+113 MB** — Engine init + Module JIT + 1 instance, one-time)       | —                            |
| RSS after 10 Sandboxes total                                          | 143.0 MB (~570 KB per additional Sandbox)                                      | —                            |
| RSS after 100 Sandboxes total                                         | 194.8 MB (~570 KB per additional Sandbox)                                      | —                            |
| RSS after 1 000 Sandboxes total                                       | 705.3 MB (~**570 KB per additional Sandbox**)                                  | —                            |
| RSS drift after 10 000 consecutive `#eval("nil")` on one Sandbox      | +2.3 MB over the whole run (~0.23 KB / invocation)                             | **0 B** per invocation       |
| Peak RSS while holding a 512 KiB return value                         | +3.6 MB above baseline                                                         | **2.5 MiB** guest `memory.grow` |
| Retained RSS after GC of the same value                               | +3.6 MB (allocator does not eagerly return pages to the OS)                    | —                            |
| Peak RSS while holding a 1 MiB capped stdout buffer                   | +4.2 MB above baseline (allocator-state-dependent)                             | **64 KiB** (stdout flows via WASI pipe, not linear memory) |
| Retained RSS after GC of the same capture                             | +3.5 MB                                                                        | —                            |

Budget ~140 MB up front per worker process plus ~570 KB per concurrent tenant; **1 000 tenants ≈ 705 MB** in one Ruby process. First-Sandbox RSS swings ~30 % run-to-run with host process load and allocator state, so treat it as a range.

## What changed vs previous baseline

Diff against the immediately previous baseline only; pre-history lives in `benchmark/results/<date>-<sha>.json` and release-tagged `benchmark/<semver>` annotated tags.

**Previous baseline:** `1be572c`, 2026-05-22. **This baseline:** `711665d`, 2026-05-27 (the 0.5.0 release line).

### Roster / schema

| Change                                          | Commit                                                                                                                          | Effect                                                                                                                                                          |
|-------------------------------------------------|---------------------------------------------------------------------------------------------------------------------------------|-----------------------------------------------------------------------------------------------------------------------------------------------------------------|
| #6 `yield_roundtrip` newly gated                | [`315f923`](https://github.com/elct9620/kobako/commit/315f923), [`c1d5559`](https://github.com/elct9620/kobako/commit/c1d5559)  | Per-yield re-entry ≈ 3.9 µs (`6c` `wall_time / 1000`)                                                                                                            |
| Suite keys renamed; characterization renumbered | [`98798c0`](https://github.com/elct9620/kobako/commit/98798c0)                                                                  | `rpc_roundtrip` → `transport_roundtrip`, `handle_table` → `catalog_handles`; concurrent #6 → **#7**, memory #7 → **#8**, `preload_dispatch` #8 → **#9**         |
| JSON gains `ips_mean` + `wall_time_sd`          | [`5034215`](https://github.com/elct9620/kobako/commit/5034215), [`0cfaebc`](https://github.com/elct9620/kobako/commit/0cfaebc)  | Capacity reading next to the median; dispersion band for the noise-aware gate                                                                                   |

### Metric deltas vs `1be572c`

| Case                       | Previous | Current  | Status                                                                       |
|----------------------------|----------|----------|------------------------------------------------------------------------------|
| `2a-empty-call`            | 136 µs   | 136 µs   | flat                                                                         |
| `4a-arith-100k`            | 43.89 ms | 42.83 ms | flat (-2 %)                                                                  |
| `5a-alloc-100_000`         | 47.90 ms | 44.24 ms | flat (-8 %)                                                                  |
| `8a-rss-after-1-sandbox`   | 191.3 MB | 137.9 MB | host process load / allocator state; inside the documented ~30 % RSS variance |
| `1b` `wall_time`           | 135 µs   | 257 µs   | single-sample `annotate_usage!` anomaly; use `2a`'s median for the split    |

All five pre-existing gated suites are flat within noise — no +10 % trip. The 0.5.0 cycle was internal refactoring (`RPC` → `Transport`, `HandleTable` → `Catalog::Handles`, codec `Encode`/`Decode` trait lift, envelope self-encoding); none of it touched the wire bytes or the hot paths.

For the `19e51d9` → `1be572c` diff (B-35 schema addition, `5a`/`5b` Handle-object alloc, `4e`/`4f` IO-write step) see git history.

## Running

```bash
bundle exec rake bench                   # six gated benchmarks (CI-friendly, payloads ≤ 1 MiB)
bundle exec rake bench:full              # adds the 16 MiB codec payload sweep
bundle exec rake bench:concurrent        # multi-Thread characterization (#7)
bundle exec rake bench:memory            # per-Sandbox RSS characterization (#8)
bundle exec rake bench:preload_dispatch  # #preload + #run characterization (#9)
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
    "captured_at": "2026-05-27T14:20:41Z"
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
| `seconds`                                            | `one_shot` and concurrent CPU seconds; wall seconds on the multi-thread suite.                |
| `wall_time` / `wall_time_sd` / `memory_peak`         | Sandbox-driven rows only (B-35). Median of `Sandbox#usage` samples; `memory_peak` is `memory.grow` delta past the per-invocation baseline. Annotate-only rows (`1b`) carry one sample with no dispersion. |

Release baselines are additionally marked with `benchmark/<semver>` annotated git tags.

## Release gate

`rake bench:gate[current,baseline]` compares a run against the committed anchor `benchmark/baseline.json` and exits non-zero on either a gated case regressed past the anchor or a gated case the anchor does not yet cover. The comparison logic lives in `tasks/support/kobako_bench_gate.rb`; `rake bench:gate_test` runs its unit tests.

A case is flagged only when its regression past the anchor clears **both** a +10 % floor (cumulative against the anchor, not the previous run) **and** a noise band of `2 × √(cv_current² + cv_baseline²)`. The noise band can only widen the bar on high-variance rows, never narrow it below the floor.

The anchor moves only via `rake bench:bless[run.json]` — re-blessing is the deliberate act of accepting a new performance level and must record the accepted shift in [What changed vs previous baseline](#what-changed-vs-previous-baseline) in the same commit. A gated case present in a run but missing from the anchor fails the gate until a re-bless records it.

**Metric per row:** sandbox-driven rows gate on `wall_time`; pure host rows (`3a-host-decode-*` / `3a-host-encode-*`) gate on median `ips`; the guest-return rows' host wrapper (`1/ips − wall_time`) is GC/allocator-bound on the largest payloads and is characterization, not a gate signal. One-shot / cold-path rows carry no dispersion and are skipped. The three characterization suites (#7 / #8 / #9) are informational and not part of the gate.

## Known caveats

- **Guest String size cap at 1 MiB.** `MRB_STR_LENGTH_MAX` is mruby's default; the guest-side codec cases stop at 512 KiB. The 16 MiB wire payload limit is reachable only through composite values.
- **Aggregate throughput is GVL-bounded.** Multi-Thread scaling stays near-flat because `ext/` does not release the GVL during wasmtime execution.
- **One-shot timings are filesystem-cache-sensitive.** The first `Sandbox.new` reads `data/kobako.wasm` from disk; cold vs hot page cache can vary 5-10 %.
- **Per-suite ordering matters.** `5c` and `8d` are sensitive to GC / allocator state built up by earlier cases in the same process; re-running a case in isolation produces different numbers.
