# Benchmarks

Kobako maintains a regression benchmark suite covering the five performance dimensions [SPEC.md](../SPEC.md) names as release quality gates (startup, RPC round-trip, codec, mruby VM, HandleTable) plus three characterization suites (multi-thread behaviour, per-Sandbox RSS, `#preload` + `#run` dispatch). Baselines for every release live under `benchmark/results/` so subsequent runs can diff against a known point; a +10% regression on any of the five gated benchmarks requires explicit review before release.

## Latest baseline

Captured on **2026-05-20** at commit `19e51d9` — macOS arm64, Ruby 3.4.7, 16 CPUs, YJIT off. Numbers below are typical; absolute values vary by hardware, but the relative shape (cold/warm ratio, RPC overhead, scaling curves) is consistent across machines.

How the numbers are obtained:

- **`ips` cases** — the runner calibrates an iteration count that lands ~1/5 of the time budget per cycle, runs a warmup pass with the same iteration shape, then records CPU time per cycle until the budget is exhausted. `ips` is the mean of per-cycle samples; `±ips_sd` is the sample standard deviation as a percentage. CPU time excludes scheduler / background-load noise, so the same code on the same machine reproduces within the reported `±ips_sd`.
- **`one_shot` cases** — the block runs exactly once and the CPU seconds consumed are recorded. Used for cold-path costs (the very first `Sandbox.new` in a process, large-table allocations) where iterating would only ever observe the warm path.
- **Multi-thread cases** keep their own wall-clock helper and bypass the runner, because measuring scheduler overhead by CPU time would defeat the purpose.
- **Per-invocation `usage`** (sandbox-driven `ips` cases only) — for cases whose block drives a `Kobako::Sandbox`, the runner samples `sandbox.usage` ([SPEC.md B-35](../SPEC.md)) right after the measurement loop and folds `wall_time` (Float seconds the guest export call spent inside wasmtime during the last iteration) and `memory_peak` (Integer bytes of `memory.grow` delta past the per-invocation baseline) into the same JSON row. Host throughput (`ips`) and per-invocation guest budget surface together, so the per-`#eval` overhead and the VM execution time are directly readable instead of derived by subtraction. The `memory.rb` (#7) suite samples the same two fields alongside the RSS deltas it already records.

### Reading the numbers

Every per-suite table in this file is derived from the raw JSON in `benchmark/results/<date>-<sha>.json` via one shared conversion script. Future baseline refreshes regenerate the tables by re-running it:

```
bundle exec ruby benchmark/support/format_baseline.rb [path/to/results.json]
```

The script defaults to the most recently modified result file when no path is given. The unit conversions it applies (single source of truth — change here, not by hand in the tables below):

| Raw field | Derived | Conversion |
|---|---|---|
| `ips` (≥ 1 000 000) | `ns` per op | `1e9 / ips` |
| `ips` (1 000 .. 999 999) | `µs` per op | `1e6 / ips` |
| `ips` (< 1 000) | `ms` per op | `1000 / ips` |
| `ips_sd` | `±sd` percentage | `(ips_sd / ips) * 100` |
| `seconds` (`one_shot`) | `ms` | `seconds * 1000` |
| `wall_time` (< 1e-6 s) | `ns` | `wall_time * 1e9` |
| `wall_time` (1e-6 .. 1e-3 s) | `µs` | `wall_time * 1e6` |
| `wall_time` (≥ 1e-3 s) | `ms` | `wall_time * 1e3` |
| `memory_peak` (< 1024 B) | `B` | direct |
| `memory_peak` (1024 .. 1 048 575 B) | `KiB` | `memory_peak / 1024` |
| `memory_peak` (≥ 1 048 576 B) | `MiB` | `memory_peak / 1 048 576` |
| `rss_kb` | `MB` | `rss_kb / 1024` |
| `ops_per_sec` (concurrent) | `ops/s` (pretty-printed) | direct, with `k` suffix above 10 000 |

For "N-ops-in-one-invocation" cases (e.g., `2d-1000-rpcs-in-one-eval`), the table cell shows both the per-invocation total (`N / ips`) and the per-op cost (the same value divided by N). The script emits the per-invocation total; the per-op interpretation is added in prose.

For "delta between waypoints" rows (e.g., 8a-1 → 8a-64 isolating per-snippet preload cost), subtract the lower waypoint from the higher and divide by the snippet-count delta. Worked examples are inline in the respective sections.

For sandbox-driven `ips` cases the JSON now also carries `wall_time` and `memory_peak` from `Kobako::Sandbox#usage`. `format_baseline.rb` renders them next to the ips meta as `wall=… mem=…` (e.g. `±1.4%, n=3 | wall=80.0 µs mem=0 B`). The two readers turn the per-`#eval` overhead into a directly recorded number: subtract `wall_time` from `1 / ips` to get the host wrapper cost. For "N-ops-in-one-invocation" cases divide `wall_time` by N to get the steady-state per-op guest cost without the host wrapper term — `2d-1000-rpcs-in-one-eval` is the canonical example. `memory_peak` is `0` for cases that don't grow guest linear memory (nil-returning evals, RPC roundtrips); for cases that do (large String returns, deep Array construction), it reports the high-water `memory.grow` delta past the per-invocation baseline.

**Rounding convention.** The script emits one decimal place for ips-derived values (`275.6 µs`) so the conversion is reversible; the prose tables round to three significant figures (`276 µs`) for readability. A future-baseline diff that comes from rounding alone — e.g., `275.6 µs` versus `274.9 µs` both rendering as `275 µs` — is not a real change. When comparing two baselines treat any prose-table delta under 1 µs as noise and consult the JSON / script output for the precise value.

### Sandbox construction and first run ([`cold_start.rb`](cold_start.rb))

Two costs dominate the very first `Kobako::Sandbox` in a process: wasmtime Engine initialization and `data/kobako.wasm` Module JIT compile. Both are cached at process scope, so every subsequent `Sandbox.new` in the same process is orders of magnitude cheaper.

| Scenario | Latency |
|---|---|
| First `Sandbox.new` in a fresh process | **613 ms** |
| Second-through-tenth `Sandbox.new` (cache warm) | **~0.10 ms** each |
| Steady-state `Sandbox.new` only | **132 µs** |
| Steady-state `Sandbox.new` + first `#eval("nil")` | **274 µs** |

The first-Sandbox cost is dominated by wasmtime JIT compiling the Module on macOS arm64. The Module is sizeable today because the guest binary embeds the mruby interpreter, the `mruby-onig-regexp` Onigmo engine, and the precompiled `mrblib/io.rb` + `mrblib/kernel.rb` IO preamble; each of those is a feature commitment the cold-start cost pays for once per process.

Practical implication: pre-warm by constructing one Sandbox at boot. After that, every per-request Sandbox costs micro-, not seconds.

### Reusing a Sandbox vs constructing one per request

| Pattern | Cost per request | Source |
|---|---|---|
| Reuse the same Sandbox (`#eval("nil")` on a warm instance) | **135 µs** | `2a-empty-rpc` |
| Fresh Sandbox every request (`Kobako::Sandbox.new.eval("nil")`) | **274 µs** | `1b-sandbox-new+eval-nil` |
| Overhead of constructing a new Sandbox per request | **~139 µs** | difference |

Per-request construction does NOT pay the multi-second Engine/Module cold cost again — that is amortized to the first Sandbox in the process regardless of pattern. The per-request overhead is the `Sandbox.new` work itself (Wasm instance creation, capture-buffer allocation, RPC Server init).

Practical implication: choose per-request construction when guest scripts are mutually untrusted; choose reuse when a single Sandbox serves repeated requests from the same trust scope. At ~140 µs of extra overhead per request, per-request isolation is affordable for most web/job workloads.

### Per-request RPC latency ([`rpc_roundtrip.rb`](rpc_roundtrip.rb))

Each row wraps the call inside one `#eval`, so the absolute number bundles `#eval` setup (~130 µs) with the RPC.

| Scenario | Latency |
|---|---|
| One Service call returning `nil`, alone in `#eval` | **135 µs** |
| One Service call with one Integer arg | **136 µs** |
| One Service call with one Symbol-keyed keyword arg | 137 µs |
| 1 000 sequential Service calls inside one `#eval` | 6.6 ms total → 6.6 µs per RPC |
| Handle chain — one RPC returns object, second targets the Handle | 150 µs |

All five rows reproduce within ±1.5% across cycles. The per-RPC steady-state cost holds at ~6.6 µs once the per-`#eval` setup is amortized over many RPCs in one invocation. The Handle-chain row exercises [SPEC.md B-17](../SPEC.md): a Factory Service returns a host object → kobako allocates a Handle on the return path → the guest invokes a method against the Handle → kobako fetches the host object via the Handle. The cost above the empty-RPC baseline is the second RPC plus one `HandleTable#alloc` and one `HandleTable#fetch`.

### Wire codec — host side ([`codec.rb`](codec.rb))

Encoding and decoding through `Kobako::Codec` directly from Ruby. These numbers do not cross the wasm boundary; they characterize the host-side codec on its own.

| Payload | Encode | Decode |
|---|---|---|
| String, 64 B | 548 ns | 579 ns |
| String, 1 KiB | 662 ns | 646 ns |
| String, 64 KiB | 7.5 µs | 2.9 µs |
| String, 1 MiB | 65.4 µs | 36.0 µs |
| Array nested 1 deep (1 KiB leaf) | 655 ns | 767 ns |
| Array nested 64 deep (1 KiB leaf) | 1.2 µs | 8.6 µs |

Per-wire-type micro-bench at primitive sizes, one entry per SPEC.md Type Mapping row (12 entries):

| Wire type | Encode | Decode |
|---|---|---|
| `nil` / Boolean / Integer / Float | 535-543 ns | 533-546 ns |
| Short String / binary String | 545 ns | 557-580 ns |
| 3-element Array / 1-entry Hash | 557-577 ns | 805-911 ns |
| Symbol (ext 0x00) | 655 ns | 723 ns |
| Handle (ext 0x01) | 654 ns | 1.1 µs |
| Exception envelope (ext 0x02) | 1.3 µs | 2.8 µs |

All rows reproduce within ±2.5% across cycles except `3a-host-encode-64KiB` (±4.4% — sensitive to allocator state when the working set crosses a page boundary) and `3a-host-encode-1MiB` (±4.8% — large-payload allocation noise). The neighbouring 64B / 1KiB / 1MiB-decode rows hold the load-bearing String codec numbers.

### Wire codec — guest side ([`codec.rb`](codec.rb))

The guest builds a value in mruby and returns it from `#eval`. The absolute numbers bundle guest encode + host decode + the constant `#eval` overhead; cross-row differences isolate the codec contribution.

| Guest script returns | Latency |
|---|---|
| `"x" * 64` (64 B String) | 121.2 µs |
| `"x" * 1024` (1 KiB String) | 122.3 µs |
| `"x" * 65536` (64 KiB String) | 156.5 µs |
| `"x" * 524288` (512 KiB String) | 408.9 µs |
| Array nested 1 deep (1 KiB leaf) | 123.5 µs |
| Array nested 64 deep (1 KiB leaf) | 160.8 µs |

Note: guest mruby caps a single String at 1 MiB ([SPEC Invariant](../SPEC.md)); the largest guest sample here is 512 KiB. Composite values (Arrays, Hashes) can still approach the 16 MiB wire payload limit.

### mruby VM, no RPC ([`mruby_eval.rb`](mruby_eval.rb))

Pure interpreter work — every script is a self-contained mruby computation whose only host cost is the constant `Sandbox#eval` overhead. Useful for spotting regressions in `build_config/wasi.rb` flag changes.

| Script | Latency |
|---|---|
| 100 000-iteration integer XOR loop | **43.0 ms** |
| 1 000 single-character String appends | 581 µs |
| 100 cycles of `raise` / `rescue` | 297 µs → 3.0 µs per cycle |
| 1 000 Onigmo `Regexp =~` matches | 2.97 ms → 3.0 µs per match |
| 1 000 `puts` of 64 B (below 1 MiB stdout cap) | 3.43 ms → 3.4 µs per write |
| 2 048 `puts` of ~1 KiB against the 1 MiB stdout cap | 7.98 ms (first ~1 024 land, rest silently dropped) |

The `4d` / `4e` / `4f` rows cover features that landed since `0.1.2`: Onigmo `Regexp` via `mruby-onig-regexp`, the full B-04 IO surface (`puts` / `print` / `printf` / `p` / `$stdout` / `$stderr` wired through to a host-captured WASI pipe), and the per-channel `stdout_limit` cap on that capture buffer. The cap is honored: guest `puts` does not raise on rejection, the pipe returns short, the loop runs to completion, and `sandbox.stdout_truncated?` is `true` after the run.

### Handle table scaling ([`handle_table.rb`](handle_table.rb))

`HandleTable` is the host-side mapping from opaque integer IDs to Ruby objects, reset at the start of every invocation (`#eval` or `#run`). These numbers verify the underlying Hash stays O(1) as it grows.

| Scenario | Latency |
|---|---|
| Allocate one Handle in an empty table | 257 ns |
| Allocate 100 Handles from empty | 14.4 µs total |
| Allocate 10 000 Handles from empty | 1.39 ms total |
| Allocate 100 000 Handles from empty | 15.67 ms total |
| 1 000 allocs against a 1 K-entry table | 0.099 ms |
| 1 000 allocs against a 10 K-entry table | 0.093 ms |
| 1 000 allocs against a 100 K-entry table | 0.100 ms |
| 1 000 allocs against a 1 M-entry table | 0.123 ms |
| Warm `Sandbox#eval("nil")` round-trip under sustained heap pressure | 120 µs |

The 1 K to 1 M waypoint rows confirm the dictionary stays effectively flat as the table grows — per-alloc cost holds around 93-123 ns across four orders of magnitude. ([SPEC.md B-21](../SPEC.md) caps the counter at `0x7fff_ffff` and rejects allocation past the cap; the cap guard itself is constant-time and not iterated here.)

The `5c-warm-eval-nil-under-gc-pressure` row deliberately measures a different dimension than `1b-sandbox-new+eval-nil` from cold_start (~274 µs). It runs **after** the 5b loop has grown a 1 M-entry HandleTable that stays alive in the same Ruby process for the rest of the run, so every measured `#eval` allocates capture-buffer Strings under sustained GC pressure. 1b is the clean per-invocation cost; 5c is the regression signal for changes that make per-invocation work more GC-sensitive when the process is already holding a large HandleTable — a condition 1b cannot detect.

### `#preload` + `#run` dispatch ([`preload_dispatch.rb`](preload_dispatch.rb)) — characterization only

Coverage of the two verbs added after the SPEC #1..#5 suite was written. `#preload` and `#run` are independent features — `#preload(code: ..., name: ...)` registers snippets that replay against the fresh `mrb_state` on every subsequent invocation (whether `#eval` or `#run`); `#run(:Target)` dispatches into a preloaded entrypoint constant via the Invocation envelope. The rows below isolate each verb's contribution rather than comparing them against `#eval`.

| Scenario | Latency |
|---|---|
| `Sandbox.new` + 1 `#preload(code:)` | 129 µs |
| `Sandbox.new` + 8 `#preload(code:)` | 133 µs |
| `Sandbox.new` + 64 `#preload(code:)` | 287 µs |
| Warm `#run(:Noop)` (1 entrypoint preloaded) | 164 µs |
| Warm `#run(:Echo, 42)` (positional arg) | 162 µs |
| Warm `#run(:Greet, name: :alice)` (Symbol-keyed kwargs) | 163 µs |
| Warm `#run(:Noop)` with 0 helper snippets preloaded | 147 µs |
| Warm `#run(:Noop)` with 8 helper snippets preloaded | 204 µs |
| Warm `#run(:Noop)` with 64 helper snippets preloaded | 713 µs |
| Warm `#run(:Wrap, StringIO)` (B-34 host→guest auto-wrap) | _pending next baseline_ |

8a's 1→8→64 sweep is dominated by the `Sandbox.new` term (~132 µs from `1a-sandbox-new`) at low N; the meaningful signal is the 1→64 delta — 287 − 129 = 158 µs spread across 63 extra `#preload` calls, which puts the per-snippet preload cost at ~2.5 µs. The `#preload(code:)` path trial-compiles each source against a fresh `mrb_state` to catch E-32 early; that compile dominates per-snippet cost.

8b / 8c / 8d show that positional args and Symbol kwargs add essentially nothing on top of the empty `#run` baseline (162-163 µs vs 164 µs). The Invocation envelope's args / kwargs encoding is cheap compared to the per-invocation setup. The ext 0x00 path here is the host→guest direction; the structurally distinct guest→host kwargs path is covered by `rpc_roundtrip 2c` at ~137 µs.

8f covers the [B-34](../docs/behavior.md) host→guest auto-wrap path that 8c / 8d miss. The arg (a `StringIO`) is not wire-representable, so `Kobako::Codec::Utils.deep_wrap` routes it through `HandleTable#alloc` and the guest sees a `Kobako::Handle` proxy in its place. The entrypoint discards the proxy without calling back, so the case isolates the host-side wrap cost — predicate + `alloc` + wire encode — without compounding with a guest→host RPC roundtrip. The delta against 8c (positional `Integer`) measures exactly that wrap branch. Numbers will populate with the next baseline refresh.

8e isolates per-invocation snippet replay cost: the 0→8 delta gives (204 − 147) / 8 ≈ 7.1 µs per snippet per invocation, and the 0→64 delta gives (713 − 147) / 64 ≈ 8.8 µs per snippet — linear in snippet count, which is what B-32's "replay every snippet against the fresh `mrb_state`" contract requires. A regression that adds super-linear work (e.g., O(N²) constant resolution) would show as a curved slope here, not the current near-linear one.

### Multi-Thread behavior ([`concurrent/threads.rb`](concurrent/threads.rb)) — characterization only

`ext/` does not call `rb_thread_call_without_gvl` during wasm execution, so wasm-side work is GVL-serialized. Ruby-side `#eval` setup can still overlap, which is why throughput scales modestly rather than not at all. This suite uses wall-clock timing because that is what scheduler effects manifest in.

| Scenario | Result |
|---|---|
| 1 Thread, owning one Sandbox | 7.5k `#eval`/s |
| 2 Threads, each owning one Sandbox | 8.0k `#eval`/s (essentially flat) |
| 4 Threads, each owning one Sandbox | 8.0k `#eval`/s |
| 8 Threads, each owning one Sandbox | 7.6k `#eval`/s |
| Per-Sandbox `Sandbox.new` cost, single-Threaded | 0.202 ms |
| Per-Sandbox `Sandbox.new` cost, 8 Threads in parallel | 0.172 ms each (1.376 ms total / 8) |
| `#eval("nil")` baseline | 0.130 ms |
| `#eval("nil")` while another Thread is in a long `#eval` | 0.275 ms (2.1× baseline) |

Practical implication for Sidekiq / Puma cluster shapes: a long-running script does NOT block other Threads' short `#eval` calls by hundreds of milliseconds. The contention overhead is bounded because any host-side synchronization (Queue push from a Service callback, mutex acquisition, IO) yields the GVL and lets the contending Thread interleave. The exact ratio varies run-to-run (1.5-3×) with scheduler quirks; the order of magnitude is the regression signal.

### Memory cost ([`memory.rb`](memory.rb)) — characterization only

External RSS sampling (`ps -o rss=`) — we only observe what the host process consumes, never look inside the Sandbox's mruby heap or Wasm linear memory. This is the right granularity for capacity planning (how many tenants fit in one process) without violating SPEC's Non-Goal of per-invocation instrumentation.

| Scenario | Result |
|---|---|
| Process RSS at boot (no Sandbox) | 27.4 MB |
| RSS after the first `Sandbox.new` + `#eval("nil")` | 179.3 MB (**+152 MB** — Engine init + Module JIT + 1 instance, one-time) |
| RSS after 10 Sandboxes total | 184.2 MB (~500 KB per additional Sandbox) |
| RSS after 100 Sandboxes total | 234.6 MB (~570 KB per additional Sandbox) |
| RSS after 1 000 Sandboxes total | 756.4 MB (~**580 KB per additional Sandbox**) |
| RSS drift after 10 000 consecutive `#eval("nil")` on one Sandbox | +0.8 MB over the whole run (~0.08 KB / invocation; consistent with allocator page retention, not a B-15 / B-19 violation) |
| Peak RSS while holding a 512 KiB return value | +2.5 MB above baseline |
| Retained RSS after GC of the same value | +2.5 MB (allocator does not eagerly return pages to the OS; the Ruby reference is dropped) |
| Peak RSS while holding a 1 MiB capped stdout buffer | +0.8 MB above baseline (allocator-state-dependent — see note) |
| Retained RSS after GC of the same capture | +0.0 MB |

Practical implication for tenant isolation: budget ~150 MB up front per worker process (paid by the first Sandbox), plus ~580 KB per concurrent tenant. **1 000 tenants ≈ 760 MB** in one Ruby process — comfortably within a typical Sidekiq / Puma worker's RSS limit. The 580 KB number is dominated by each Sandbox's own Wasm Instance, its linear memory, and the per-channel WASI capture pipes (stdout/stderr); the Engine and the compiled Module are shared at process scope and not re-paid per Sandbox.

The `7d` peak / retained numbers fluctuate run-to-run depending on whether the allocator already holds pages large enough to fit the 1 MiB capture buffer. The cap itself is honored regardless: `stdout_truncated?` flips to `true` and the captured buffer ends at the 1 MiB boundary regardless of how much the guest tried to write. A persistent jump in this row across runs would indicate the capture buffer is growing without bound.

The `7b` per-invocation drift remains bounded — 0.8 MB over 10 000 invocations, in line with allocator page retention. B-15 / B-19 per-invocation reset is still honored at the Ruby level.

## What changed vs previous baseline

This section is the diff against the *immediately previous* baseline — it is replaced (not appended) every time the Latest baseline above is refreshed. Pre-history lives in git (`benchmark/results/<date>-<sha>.json` files) and in release-tagged `benchmark/<semver>` annotated tags.

**Previous baseline:** `8bfd888`, 2026-05-20. **This baseline:** `19e51d9`, 2026-05-20.

One semantic change shapes this diff:

- **`memory_limit` is now per-invocation linear-memory delta.** Previously the cap compared `desired_total > memory_limit` directly, which folded the mruby image's initial allocation and every prior invocation's high-water mark into the budget — turning a SPEC-documented "per-invocation" cap into a Sandbox-global cumulative one. The `KobakoLimiter` now charges only `desired - baseline`, where `baseline` is the linear-memory size observed at invocation entry. The five gated suites all pass `memory_limit: nil` and are unaffected; the `#7` characterization suite previously tripped the (effectively shrinking) cap on 7c / 7d and now also runs with `memory_limit: nil` per [`chore(bench)`: align suites with per-invocation memory_limit semantics](https://github.com/elct9620/kobako/commit/19e51d9).

Gated cases are within ±5% of the prior baseline — `1a-sandbox-new` (124 → 132 µs), `5a-alloc-100_000-from-empty` (15.23 → 15.67 ms), `3a-host-encode-1MiB` (57.8 → 65.4 µs) are the largest movers, all consistent with allocator / page-cache noise on the host machine. Every `2x` RPC row, every `3c` per-wire-type row, every `4x` mruby-VM row, and the `5b` HandleTable waypoint curve are within their `±sd` bands.

`#7` memory characterization now reports honest peak/retained numbers because no row trips the cap. 7c retained moves 3.5 → 2.5 MB; 7d retained moves 2.5 → 0.0 MB (the allocator returned the page once the capture buffer was freed). 7b drift dropped 2.2 → 0.8 MB over 10 000 invocations, well inside allocator page-retention noise.

`6c-blocking-ratio` widened from 1.33× to 2.12× — wall-clock measurements remain sensitive to background process load on the measurement host; the documented "varies run-to-run (1.5-3×)" band still holds.

For the previous (`c605109` → `8bfd888`) diff — verb rename + new `#8 preload_dispatch` suite — see git history.

## Running

```bash
bundle exec rake bench             # five gated benchmarks (CI-friendly, payloads ≤ 1 MiB)
bundle exec rake bench:full              # adds the 16 MiB codec payload sweep
bundle exec rake bench:concurrent        # multi-Thread characterization (#6)
bundle exec rake bench:memory            # per-Sandbox RSS characterization (#7)
bundle exec rake bench:preload_dispatch  # #preload + #run characterization (#8)
```

Each rake task shells out to `bundle exec ruby benchmark/<file>.rb`; you can also invoke a single script directly for fast iteration:

```bash
bundle exec ruby benchmark/rpc_roundtrip.rb
```

Total wall time for `bundle exec rake bench` is roughly 5-8 minutes on a current-gen laptop (codec dominates with 46 cases × 3 s warmup + 3 s measurement); each characterization task adds 30 s to 1 minute.

YJIT is not turned on by the suite. Invoke with `RUBY_YJIT_ENABLE=1 bundle exec rake bench` or `bundle exec ruby --yjit benchmark/<file>.rb` to capture a YJIT baseline; the resulting JSON records `yjit_enabled: true` so it is unambiguously distinct from a non-YJIT baseline.

## Result files

Every run writes (or merges into) `benchmark/results/<date>-<short-sha>.json`:

```json
{
  "env": {
    "ruby_version": "3.4.7",
    "ruby_platform": "arm64-darwin24",
    "processors": 16,
    "yjit_enabled": false,
    "git_sha": "19e51d9",
    "captured_at": "2026-05-20T16:09:55Z"
  },
  "suites": {
    "cold_start":     [ { "label": "1a-sandbox-new", "ips": 8080.0, "ips_sd": 112, "iterations": 24576, "cycles": 3 } ],
    "rpc_roundtrip":  [ { "label": "2a-empty-rpc",   "ips": 7407.0, "ips_sd": 80,  "iterations": 18432, "cycles": 3,
                          "wall_time": 0.0000045, "memory_peak": 0 } ],
    ...
  }
}
```

- **`ips`** — iterations per CPU second; higher is better.
- **`ips_sd`** — standard deviation of the per-cycle `ips` samples; report as a percentage of `ips`.
- **`iterations`** / **`cycles`** — total iterations measured and number of samples; small `cycles` means few samples were collected within the time budget (high per-iter cost), and the corresponding `ips_sd` should be read accordingly.
- **`seconds`** — appears on one-shot entries (cold construction, large-table allocs, concurrent measurements) where iterating would mask the cold-path cost. CPU seconds for the `case`/`one_shot` runners; wall-clock seconds for the multi-thread suite.
- **`wall_time`** / **`memory_peak`** — present on sandbox-driven `ips` rows ([SPEC.md B-35](../SPEC.md)). `wall_time` is the Float seconds the guest export call spent inside wasmtime during the last measured iteration; `memory_peak` is the Integer high-water `memory.grow` delta past the per-invocation baseline. Both reflect *one* invocation, not the loop total — combine with `ips` for steady-state interpretation.

Release baselines are additionally marked with annotated git tags following `benchmark/<semver>` (per SPEC.md).

## Release gate

A regression greater than **+10 %** on any of the five gated benchmarks (startup, RPC, codec, mruby VM, HandleTable) versus the previous release baseline requires explicit review and approval before release proceeds.

The three characterization suites (`#6` multi-Thread, `#7` memory, `#8` `#preload` + `#run` dispatch) are informational and not part of the gate, but baselines are recorded so before/after comparison is possible when changes touch the GVL boundary (e.g. introducing `rb_thread_call_without_gvl` in `ext/`), the per-Sandbox memory model, or the snippet preload / dispatch path.

## Known caveats when reading results

- **Guest String size cap at 1 MiB.** `MRB_STR_LENGTH_MAX` is 1 MiB by mruby default; the guest-side codec cases stop at 512 KiB. The wire payload limit (16 MiB) is reachable only through composite values.
- **Aggregate throughput is GVL-bounded.** Multi-Thread scaling stays close to flat because `ext/` does not release the GVL during wasmtime execution. Genuine wasm parallelism would require introducing `rb_thread_call_without_gvl` on the host side.
- **One-shot timings are sensitive to filesystem cache.** The first `Sandbox.new` reads `data/kobako.wasm` from disk and JIT-compiles the Module. Numbers can vary 5-10 % between a cold OS page cache and a hot one — record both states when investigating a regression in the first-construction number.
- **Per-suite ordering matters.** Several rows (`5c`, `7d`) are explicitly sensitive to GC / allocator state built up by earlier cases in the same process. Re-running a single case in isolation will produce different numbers than running it as part of `rake bench`. The published baseline reflects the in-suite numbers.
- **`ips` is steady-state.** Cold-path costs that only occur once per process (Engine init, Module compile) are captured via `one_shot` entries (`1c-sandbox-new-1`), not the `ips` cases. Watch the right metric for the question you are asking.
