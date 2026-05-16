# Benchmarks

Kobako maintains a regression benchmark suite covering the five performance dimensions [SPEC.md](../SPEC.md) names as release quality gates (startup, RPC round-trip, codec, mruby VM, HandleTable) plus two characterization suites (concurrency, per-Sandbox RSS). Baselines for every release live under `benchmark/results/` so subsequent runs can diff against a known point; a +10% regression on any of the five gated benchmarks requires explicit review before release.

## Latest baseline

Captured on **2026-05-16** at commit `deb7c9d` — macOS arm64, Ruby 3.4.7, 16 CPUs. Numbers below are typical; absolute values vary by hardware, but the relative shape (cold/warm ratio, RPC overhead, scaling curves) is consistent across machines.

This baseline supersedes the 2026-05-13 / `f4da86e` snapshot. Three feature lines landed between the two captures and shift the absolute numbers (the relative shape across cases is unchanged); see [What changed since 2026-05-13](#what-changed-since-2026-05-13) below.

### Sandbox construction and first run ([`cold_start.rb`](cold_start.rb))

Two costs dominate the very first `Kobako::Sandbox` in a process: wasmtime Engine initialization and `data/kobako.wasm` Module JIT compile. Both are cached at process scope, so every subsequent `Sandbox.new` in the same process is orders of magnitude cheaper.

| Scenario | Latency |
|---|---|
| First `Sandbox.new` in a fresh process | **602 ms** |
| Second-through-tenth `Sandbox.new` (cache warm) | **~0.10 ms** each (~6 000× faster) |
| Steady-state `Sandbox.new` only | **121 µs** |
| Steady-state `Sandbox.new` + first `#run("nil")` | **267 µs** |

Practical implication: pre-warm by constructing one Sandbox at boot. After that, every per-request Sandbox costs micro-, not milliseconds.

### Reusing a Sandbox vs constructing one per request

Two execution shapes dominate real use. The first is the "setup-once, run-many" pattern from [README.md](../README.md): one Sandbox per scope, many `#run` calls on it. The second is the per-request construction pattern needed for hard tenant isolation (one Sandbox per request, per submission, or per untrusted script — the shape of journeys J-03 and J-04 in SPEC.md), where every request pays the construction cost.

| Pattern | Cost per request | Source |
|---|---|---|
| Reuse the same Sandbox (`#run("nil")` on a warm instance) | **136 µs** | `2a-empty-rpc` baseline |
| Fresh Sandbox every request (`Kobako::Sandbox.new.run("nil")`) | **267 µs** | `1b-sandbox-new+run-nil` |
| Overhead of constructing a new Sandbox per request | **~131 µs / req (~2.0×)** | difference |

The overhead breaks down as ~121 µs `Sandbox.new` (Wasm instance creation, capture-buffer allocation, RPC Server init — Engine and Module are cached at process scope so this is the warm path) plus the per-`#run` setup that both patterns pay. Per-request construction does NOT pay the 602 ms cold Engine/Module cost again — that cost is amortized to the first Sandbox in the process regardless of pattern.

Practical implication: choose per-request construction when guest scripts are mutually untrusted (so capability state and Handle leaks between requests are unacceptable); choose reuse when a single Sandbox serves repeated requests from the same trust scope. At ~131 µs of extra overhead per request, per-request isolation is affordable for most web/job workloads.

### Per-request RPC latency ([`rpc_roundtrip.rb`](rpc_roundtrip.rb))

Each row wraps the call inside one `#run`, so the absolute number bundles `#run` setup (~130 µs) with the RPC. The 1000-call row amortizes that overhead by making the calls inside a single `#run`, which is the right number to compare against.

| Scenario | Latency |
|---|---|
| One Service call returning `nil`, alone in `#run` | **136 µs** |
| One Service call with one Integer arg | **137 µs** |
| One Service call with one Symbol-keyed keyword arg | **138 µs** |
| 1 000 sequential Service calls inside one `#run` | 6.52 ms total → **6.5 µs per RPC** |
| Handle chain — one RPC returns object, second targets the Handle | **150 µs** (`2e-handle-chain`) |

The Handle-chain row is the per-`#run` overhead plus two RPCs (a Factory call that allocates a Handle on the return path, followed by a method invocation against that Handle). The ~15 µs gap above `2a-empty-rpc` is the second RPC alone — `HandleTable#alloc` on the return path and `HandleTable#fetch` on the call path are well below 1 µs each ([SPEC.md B-17](../SPEC.md)).

### Wire codec — host side ([`codec.rb`](codec.rb))

Encoding and decoding through `Kobako::Codec` directly from Ruby. These numbers do not cross the wasm boundary; they characterize the host-side codec on its own.

| Payload | Encode | Decode |
|---|---|---|
| String, 64 B | 537 ns | 578 ns |
| String, 1 KiB | 636 ns | 638 ns |
| String, 64 KiB | 8.4 µs | 2.9 µs |
| String, 1 MiB | 61 µs | 36 µs |
| Array nested 1 deep (1 KiB leaf) | 643 ns | 758 ns |
| Array nested 64 deep (1 KiB leaf) | 1.1 µs | 8.6 µs |

Per-wire-type micro-bench at primitive sizes (mostly to detect a regression in a single type's codec path; absolute numbers cluster around 540 ns):

| Wire type | Encode | Decode |
|---|---|---|
| `nil` / Boolean / Integer / Float | ~530 ns | ~540 ns |
| Short String / binary String | ~545 ns | ~570 ns |
| 3-element Array / 1-entry Hash | ~560 ns | ~860 ns |
| Symbol (ext 0x00) | 616 ns | 716 ns |
| Handle (ext 0x01) | 654 ns | 1.1 µs |
| Exception envelope (ext 0x02) | 1.3 µs | 2.9 µs |

### Wire codec — guest side ([`codec.rb`](codec.rb))

The guest builds a value in mruby and returns it from `#run`. The absolute numbers bundle guest encode + host decode + the constant `#run` overhead; cross-row differences isolate the codec contribution.

| Guest script returns | Latency |
|---|---|
| `"x" * 64` (64 B String) | 122 µs |
| `"x" * 1024` (1 KiB String) | 122 µs |
| `"x" * 65536` (64 KiB String) | 165 µs |
| `"x" * 524288` (512 KiB String) | 423 µs |
| Array nested 1 deep (1 KiB leaf) | 123 µs |
| Array nested 64 deep (1 KiB leaf) | 173 µs |

Note: guest mruby caps a single String at 1 MiB ([SPEC Invariant](../SPEC.md)); the largest guest sample here is 512 KiB. Composite values (Arrays, Hashes) can still approach the 16 MiB wire payload limit.

### mruby VM, no RPC ([`mruby_eval.rb`](mruby_eval.rb))

Pure interpreter work. Useful for spotting performance regressions in `build_config/wasi.rb` flag changes.

| Script | Latency |
|---|---|
| 100 000-iteration integer XOR loop | **44 ms** |
| 1 000 single-character String appends | 579 µs |
| 100 cycles of `raise` / `rescue` | 299 µs → **3.0 µs per cycle** |
| 1 000 Onigmo `Regexp =~` matches | 3.10 ms → **3.1 µs per match** |
| 1 000 `puts` of 64 B (~65 KiB total, below 1 MiB stdout cap) | 4.02 ms → **4.0 µs per write** |
| 2 048 `puts` of ~1 KiB against the 1 MiB stdout cap | 16.3 ms (first ~1 024 land, rest are dropped) |

The last three rows are introduced in the 2026-05-16 baseline:

- `4d-regexp-match-1000` exercises `mruby-onig-regexp` (Onigmo engine added to `build_config/wasi.rb`). At ~3.1 µs per match the Regexp path is bounded by Onigmo, not by `#run` setup.
- `4e-stdout-puts-1000` exercises the full B-04 IO path: `mrblib/io.rb` `IO#write` → `kobako_io_fwrite` C bridge → WASI pipe → host capture buffer. At ~4.0 µs per buffered write the IO bridge is bounded by the WASI pipe enqueue, not by the C shim.
- `4f-stdout-cap-saturation` writes ~2 MiB against the default 1 MiB `stdout_limit`. Guest `puts` does not raise on cap rejection — the WASI pipe returns short, the loop runs to completion, and `sandbox.stdout_truncated?` is `true` after the run. High `±ips_sd` on this case is expected: each iteration allocates and releases ~1 MiB of captured bytes, so GC pause timing dominates the variance.

### Handle table scaling ([`handle_table.rb`](handle_table.rb))

`HandleTable` is the host-side mapping from opaque integer IDs to Ruby objects, reset at the start of every `#run`. These numbers verify the underlying Hash stays O(1) as it grows.

| Scenario | Latency |
|---|---|
| Allocate one Handle in an empty table | 264 ns |
| Allocate 100 Handles from empty | 24 µs total |
| Allocate 10 000 Handles from empty | 6.3 ms total |
| Allocate 100 000 Handles from empty | 73 ms total |
| 1 000 allocs against a 1 K-entry table | 0.55 ms |
| 1 000 allocs against a 10 K-entry table | 0.20 ms |
| 1 000 allocs against a 100 K-entry table | 0.69 ms |
| 1 000 allocs against a 1 M-entry table | 0.52 ms |
| Warm `Sandbox#run("nil")` round-trip (includes per-run reset) | 519 µs |

The middle four rows still confirm the Hash stays O(1) — the inter-row spread is GC-pause variance, not dictionary degradation. Per-alloc cost holds at ~200–700 ns across four orders of magnitude of table size.

The `5c-warm-run-nil-roundtrip` row is the slowest case in this suite by an order of magnitude. That number is GC-amplified: it executes after `5b` has grown a 1 M-entry HandleTable that stays alive in the same Ruby process, so each measured `#run` allocates capture-buffer Strings under heavy heap pressure. The fairer per-`#run` cost is in `cold_start.rb`'s `1b-sandbox-new+run-nil` (267 µs) and `rpc_roundtrip.rb`'s `2a-empty-rpc` (136 µs); `5c` is preserved here as the regression guard against changes that make `#run` more GC-sensitive than today.

### Multi-Thread behavior ([`concurrent/threads.rb`](concurrent/threads.rb)) — characterization only

`ext/` does not call `rb_thread_call_without_gvl` during wasm execution, so wasm-side work is GVL-serialized. Ruby-side `#run` setup can still overlap, which is why throughput scales modestly rather than not at all.

| Scenario | Result |
|---|---|
| 1 Thread, owning one Sandbox | ~14k `#run`/s |
| 2 Threads, each owning one Sandbox | ~14k `#run`/s (essentially flat) |
| 4 Threads, each owning one Sandbox | ~14k `#run`/s |
| 8 Threads, each owning one Sandbox | ~14k `#run`/s |
| Per-Sandbox `Sandbox.new` cost, single-Threaded | 0.146 ms |
| Per-Sandbox `Sandbox.new` cost, 8 Threads in parallel | 0.113 ms (no mutex contention on the cache) |
| `#run("nil")` baseline | 0.067 ms |
| `#run("nil")` while another Thread is in a long `#run` | 0.190 ms (1.5-3× baseline depending on the OS scheduler) |

Practical implication for Sidekiq / Puma cluster shapes: a long-running script does NOT block other Threads' short `#run` calls by hundreds of milliseconds. The contention overhead is bounded because any host-side synchronization (Queue push from a Service callback, mutex acquisition, IO) yields the GVL and lets the contending Thread interleave. The exact ratio varies run-to-run (1.5-3×) with scheduler quirks; the order of magnitude is the regression signal.

### Memory cost ([`memory.rb`](memory.rb)) — characterization only

External RSS sampling (`ps -o rss=`) — we only observe what the host process consumes, never look inside the Sandbox's mruby heap or Wasm linear memory. This is the right granularity for capacity planning (how many tenants fit in one process) without violating SPEC's Non-Goal of per-`#run` instrumentation.

| Scenario | Result |
|---|---|
| Process RSS at boot (no Sandbox) | 25 MB |
| RSS after the first `Sandbox.new` + `#run("nil")` | 166 MB (**+140 MB** — Engine init + Module JIT + 1 instance, one-time) |
| RSS after 10 Sandboxes total | 171 MB (~560 KB per additional Sandbox) |
| RSS after 100 Sandboxes total | 220 MB (~560 KB per additional Sandbox) |
| RSS after 1 000 Sandboxes total | 738 MB (~**585 KB per additional Sandbox**) |
| RSS drift after 10 000 consecutive `#run("nil")` on one Sandbox | +1.4 MB over the whole run (~0.14 KB / run; consistent with allocator page retention, not a B-15 / B-19 violation) |
| Peak RSS while holding a 512 KiB return value | +3.6 MB above baseline |
| Retained RSS after GC of the same value | +3.6 MB (allocator does not eagerly return pages to the OS; the Ruby reference is dropped) |
| Peak RSS while holding a 1 MiB capped stdout buffer | +3.2 MB above baseline (`7d-rss-while-holding-near-cap-stdout`) |
| Retained RSS after GC of the same capture | +2.4 MB (WASI pipe buffer + `Sandbox#stdout` String not eagerly released) |

Practical implication for tenant isolation: budget ~140 MB up front per worker process (paid by the first Sandbox), plus ~580 KB per concurrent tenant. **1 000 tenants ≈ 740 MB** in one Ruby process — comfortably within a typical Sidekiq / Puma worker's RSS limit. The 580 KB number is dominated by each Sandbox's own Wasm Instance, its linear memory, and the per-channel WASI capture pipes (stdout/stderr); the Engine and the compiled Module are shared at process scope and not re-paid per Sandbox.

The 7b drift is allocator behaviour, not a real leak — the per-`#run` HandleTable reset is honored at the Ruby level; the residual RSS is malloc pages held for reuse. If a host operator needs to bound a long-lived process tightly, monitor RSS over wall-clock hours rather than per-run growth.

The 7d row documents that a saturated 1 MiB stdout cap keeps ~3 MB of RSS alive across the next `#run` and ~2 MB after the Sandbox reference is dropped — the OS allocator holds the pages for reuse rather than returning them. The cap itself is honored: `stdout_truncated?` flips to `true` and the captured buffer ends at the 1 MiB boundary regardless of how much the guest tried to write.

## What changed since 2026-05-13

Three feature lines landed between the `f4da86e` snapshot (2026-05-13) and this baseline (`deb7c9d`, 2026-05-16). Each adds work to a path the prior baseline never exercised, so the absolute numbers shift even though the relative shape across cases is unchanged.

- **B-04 IO surface** — `Kobako::Sandbox#stdout` / `#stderr` are wired end-to-end. Every `#run` now allocates per-channel `Kobako::Capture` value objects, reads both WASI pipes back to the host, and the guest binary embeds a precompiled `mrblib/io.rb` + `mrblib/kernel.rb` preamble for `puts` / `print` / `printf` / `p` / `IO#<<` / `$stdout` / `$stderr`. See `wasm/kobako-wasm/src/kobako/io.rs` and `lib/kobako/capture.rb`.
- **B-01 timeout + memory_limit caps** — `Sandbox.new` now defaults to a 60-second per-`#run` wall-clock deadline and a 5 MiB guest-memory ceiling. Each `#run` installs the wall-clock check and the `memory.grow` cap on the wasmtime store before invoking the guest. See `lib/kobako/sandbox.rb`.
- **Guest `Regexp` via mruby-onig-regexp** — Onigmo is linked into the guest Module and adds to its size; cold-start cost rises accordingly. The guest can now match `=~` / `String#match` / `Regexp.new` without raising `NoMethodError`.

The expected and observed deltas vs the prior baseline:

| Dimension | Old | New | Delta | Root cause |
|---|---|---|---|---|
| `1c-sandbox-new-1` (first Sandbox cold) | 413 ms | 602 ms | +46 % | Larger Module (IO mrblib + Onigmo) → longer wasmtime JIT compile |
| `1a-sandbox-new` (steady-state warm) | 87 µs | 121 µs | +39 % | Extra ext setup: timeout / memory_limit / per-channel WASI pipes |
| `1b-sandbox-new+run-nil` | 174 µs | 267 µs | +53 % | `1a` + per-`#run` capture read & `Capture.from_ext` × 2 |
| `2a-empty-rpc` | 76 µs | 136 µs | +78 % | The +60 µs per-`#run` overhead dominates a single-RPC call |
| `2d-1000-rpcs / per-RPC` | 5.3 µs | 6.5 µs | +23 % | Marginal per-RPC cost; the per-`#run` overhead amortises out |
| `4c-exception-raise-rescue-100` | 4.20 k ips | 3.34 k ips | −20 % | Per-`#run` overhead × benchmark-ips loop count |
| `7a-rss-after-1-sandbox` | 135 MB | 166 MB | +23 % | Onigmo + IO mrblib in the guest image |
| `7a-per-additional-sandbox` | ~215 KB | ~585 KB | +172 % | Per-Sandbox WASI capture pipes (stdout + stderr) pre-allocated |

The codec primitive-type path stays within ±2 % of the prior baseline (`3a-host-encode-1MiB` +5.5 %, `3a-host-decode-1MiB` −1.1 %). The ext-type path (`3c-host-{encode,decode}-{symbol,handle,exception}`) is uniformly ~25 % slower than the prior baseline despite the `Wire::Codec` → `Kobako::Codec` lift being pure rename; this is the one place where the regression is not fully explained by new functionality and is worth a follow-up investigation against `lib/kobako/codec/factory.rb` (now backed by stdlib `Singleton`).

The `5b-alloc-1000-at-size-*` and `5c-warm-run-nil-roundtrip` rows widened by 2–5× vs the prior baseline — this is GC-pause variance from the new per-`#run` capture-buffer allocations interacting with the long-lived 1 M-entry `batch_table` left behind by `5b`. The cleaner per-`#run` cost is in `1b-sandbox-new+run-nil` and `2a-empty-rpc`. `HandleTable` itself remains O(1); the per-alloc cost has not changed.

The `7b` per-`#run` drift remains bounded — 1.4 MB over 10 000 runs, in line with allocator page retention. B-15 / B-19 per-`#run` reset is still honored at the Ruby level.

## Running

```bash
bundle exec rake bench             # five gated benchmarks (= bench:smoke; CI-friendly, payloads ≤ 1 MiB)
bundle exec rake bench:full        # adds the 16 MiB codec payload sweep
bundle exec rake bench:concurrent  # multi-Thread characterization
bundle exec rake bench:memory      # per-Sandbox RSS characterization
```

Each rake task shells out to `bundle exec ruby benchmark/<file>.rb`; you can also invoke a single script directly for fast iteration:

```bash
bundle exec ruby benchmark/rpc_roundtrip.rb
```

Total wall time for `bundle exec rake bench` is roughly 5-7 minutes on a current-gen laptop (codec dominates with 46 cases × 3 s warm + 3 s measurement); `rake bench:concurrent` adds ~30 s.

## Result files

Every run writes (or merges into) `benchmark/results/<date>-<short-sha>.json`:

```json
{
  "env": {
    "ruby_version": "3.4.7",
    "ruby_platform": "arm64-darwin24",
    "processors": 16,
    "git_sha": "f4da86e",
    "captured_at": "2026-05-13T13:49:00Z"
  },
  "suites": {
    "cold_start":   [ { "label": "1a-sandbox-new", "ips": 11391.4, "ips_sd": 854, ... } ],
    "rpc_roundtrip": [ ... ],
    ...
  }
}
```

- **`ips`** comes from `benchmark-ips` — iterations per second; higher is better.
- **`ips_sd`** is the standard deviation across measurement cycles.
- **`seconds`** appears on one-shot entries (cold construction, large-table allocs, concurrent measurements) where iterating would mask the cold-path cost.

Release baselines are additionally marked with annotated git tags following `benchmark/<semver>` (per SPEC.md).

## Release gate

A regression greater than **+10%** on any of the five gated benchmarks (startup, RPC, codec, mruby VM, HandleTable) versus the previous release baseline requires explicit review and approval before release proceeds.

The multi-Thread benchmark is informational — its results depend on the OS scheduler and are not part of the gate, but baselines are recorded so before/after comparison is possible when changes touch the GVL boundary (e.g. introducing `rb_thread_call_without_gvl` in `ext/`).

## Known caveats when reading results

- **Guest String size cap at 1 MiB.** `MRB_STR_LENGTH_MAX` is 1 MiB by mruby default; the guest-side codec cases stop at 512 KiB. The wire payload limit (16 MiB) is reachable only through composite values.
- **Aggregate throughput is GVL-bounded.** Multi-Thread scaling caps around 1.4× from extra Ruby-side overlap. Genuine wasm parallelism would require `ext/` to release the GVL during wasmtime execution, which is currently not done.
- **One-shot timings are sensitive to filesystem cache.** The first `Sandbox.new` reads `data/kobako.wasm` from disk and JIT-compiles the Module. Numbers can vary 5-10% between a cold OS page cache and a hot one — record both states when investigating a regression in the first-construction number.
- **`benchmark-ips` measures steady-state.** Cold-path costs that only occur once per process (Engine init, Module compile) are captured via one-shot measurements, not the `ips` cases. Watch the right metric for the question you are asking.
