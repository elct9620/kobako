# Benchmarks

The kobako benchmark suite measures the five performance dimensions that [SPEC.md](../SPEC.md) names as release quality gates, plus one extra characterization benchmark covering concurrent multi-Thread usage. Baselines for every release are committed under `benchmark/results/`.

## Suite

| # | File | What it measures | Release gate |
|---|------|------------------|--------------|
| 1 | [`cold_start.rb`](cold_start.rb) | `Kobako::Sandbox.new` and `Sandbox.new + first #run`. Includes 1c — the first 10 `Sandbox.new` calls in a fresh process, demonstrating the wasmtime Engine / Module cache amortization that [README.md](../README.md) advertises. | ✅ |
| 2 | [`rpc_roundtrip.rb`](rpc_roundtrip.rb) | Service-call latency: empty RPC, primitive arg, kwargs (Symbol ext 0x00), and 1000 sequential RPCs in one `#run` to amortize the `#run` setup cost away. | ✅ |
| 3 | [`codec.rb`](codec.rb) | Host-side `Wire::Codec` encode/decode at varying payload sizes (64 B → 1 MiB) and nesting depths (1 → 64), plus a per-wire-type micro-bench covering the 11 representable types in SPEC's Type Mapping. Guest-side codec is sampled via `Sandbox#run` returning a constructed value. | ✅ |
| 4 | [`mruby_eval.rb`](mruby_eval.rb) | Pure mruby VM work with no RPC: arithmetic loop (100k iterations), string append loop (1000 ops), exception raise/rescue (100 cycles). Sensitive to changes in `build_config/wasi.rb`. | ✅ |
| 5 | [`handle_table.rb`](handle_table.rb) | `HandleTable#alloc` throughput. 5a sweeps cumulative cost from an empty table; 5b samples marginal cost at 1K / 10K / 100K / 1M waypoints to detect dictionary degradation; 5c measures the warm `#run("nil")` round-trip (includes per-run reset). | ✅ |
| 6 | [`concurrent/threads.rb`](concurrent/threads.rb) | Multi-Thread characterization. 6a runs N Threads × independent Sandboxes; 6b stresses the shared `MODULE_CACHE` mutex via concurrent `Sandbox.new`; 6c measures contention overhead when one Thread is in a long `#run` and another tries to start its own (with a host-bound `Sync::Ready` Service as a real synchronization barrier). | ❌ characterization only |

## Running

```bash
bundle exec rake bench             # #1..#5 (= bench:smoke; CI-friendly, payloads ≤ 1 MiB)
bundle exec rake bench:full        # adds the 16 MiB codec payload sweep
bundle exec rake bench:concurrent  # #6 only
```

Each rake task shells out to `bundle exec ruby benchmark/<file>.rb`; you can also invoke a single script directly for fast iteration:

```bash
bundle exec ruby benchmark/rpc_roundtrip.rb
```

## Results

Every run writes (or merges into) `benchmark/results/<date>-<short-sha>.json`:

```json
{
  "env": {
    "ruby_version": "3.4.7",
    "ruby_platform": "arm64-darwin24",
    "processors": 16,
    "git_sha": "55ee78b",
    "captured_at": "2026-05-13T12:50:00Z"
  },
  "suites": {
    "cold_start":   [ { "label": "1a-sandbox-new", "ips": 10849.4, "ips_sd": 951, ... } ],
    "rpc_roundtrip": [ ... ],
    ...
  }
}
```

- **`ips`** comes from `benchmark-ips` — higher is better.
- **`ips_sd`** is the standard deviation across measurement cycles.
- **`seconds`** appears on one-shot entries (1c, 5b, 6a/6b/6c) where iterating would mask the cold-path cost.

Files are kept in `benchmark/results/`. Release baselines are also marked with annotated git tags following `benchmark/<semver>` (per SPEC.md).

## Release gate

SPEC.md mandates: regressions greater than **+10%** on any of benchmarks **#1..#5** versus the previous release baseline require explicit review and approval before release proceeds. Benchmark **#6** is informational — its results vary with the OS scheduler and are not part of the gate, but baselines are recorded so before/after comparison is possible when changes touch the GVL boundary (e.g. introducing `rb_thread_call_without_gvl` in `ext/`).

## Known caveats

- **Guest String cap at 1 MiB (SPEC Invariant).** `MRB_STR_LENGTH_MAX` is 1 MiB by mruby default. The wire payload limit is 16 MiB, but a single guest-side String value cannot reach it — `codec.rb` caps `3a-guest-return-*` at 512 KiB. Composite values (Array, binary) can still approach the wire limit.
- **GVL bounds aggregate throughput (SPEC B-22).** `ext/` does not call `rb_thread_call_without_gvl` during wasm execution. `6a` shows modest 1.3-1.4× throughput scaling from 1 → 8 Threads (Ruby-side setup overlaps) but never linear scaling. `6c` ratio is small (1-3×) because GVL-yielding primitives (e.g. `Queue#<<` inside the `Sync::Ready` Service) let the contending Thread interleave.
- **`benchmark-ips` warms with 1s + measures 3s per case.** Total runtime for `rake bench` is ~5-7 minutes on a current-gen laptop; `rake bench:concurrent` adds ~30s. For smoke iteration during development, invoke individual scripts.
- **Cold-path entries (1c) are sensitive to filesystem cache.** The first `Sandbox.new` reads `data/kobako.wasm` from disk and JIT-compiles the Module. Numbers vary 5-10% between a cold OS page cache and a hot one — record both states when investigating a regression.
