# frozen_string_literal: true

# SPEC.md "Regression benchmarks" #6 — Yield round-trip latency.
# Detects regressions on the host-initiated re-entry path that #2
# (guest-initiated Request/Response) does not exercise: the YieldResponse
# codec, the `__kobako_yield_to_block` dispatch, and the guest-side
# BLOCK_STACK push/pop (docs/behavior.md B-23..B-30).
#
#   6a — Single yield: Service yields once, block returns its arg
#        (tag 0x01 ok). The one-yield latency above the no-block #2
#        baseline.
#   6b — Block given, never yielded: the call site supplies a block so
#        block_given travels and the host constructs a Yielder, but the
#        Service never invokes it (B-30). Isolates the block-flag +
#        Yielder construction/invalidation floor with zero re-entry.
#   6c — 1000 yields in one dispatch (the J-06 iteration shape): per-yield
#        steady-state cost once the per-dispatch setup is amortized, the
#        dimension SPEC.md #6 requires. Per-yield cost is wall_time / 1000.
#   6d — Break unwind: the block runs `break` on the first yield
#        (tag 0x02), unwinding the Service via catch/throw (B-25). The
#        delta over 6a isolates the break classification + unwind path.
#
# Every case wraps one #eval per iteration; the absolute number includes
# the constant per-invocation overhead term (see #1 1b). Regression
# detection is on the *delta* between cases, not on the absolute ips of
# any single case.

$LOAD_PATH.unshift File.expand_path("../lib", __dir__)
$LOAD_PATH.unshift File.expand_path("support", __dir__)

require "kobako"
require "runner"

runner = Kobako::Bench::Runner.new("yield_roundtrip")

# memory_limit: nil — see benchmark/transport_roundtrip.rb. This suite
# measures yield re-entry throughput, so we keep the per-invocation
# memory limiter callback out of the wasmtime hot loop.
sandbox = Kobako::Sandbox.new(memory_limit: nil)
sandbox.define(:Bench)
       .bind(:YieldOnce, ->(x, &blk)     { blk.call(x) })
       .bind(:Ignore,    ->(*, &_blk)    {})
       .bind(:MapEach,   ->(items, &blk) { items.map(&blk) })
       .bind(:EachBreak, ->(items, &blk) { items.each(&blk) })

# Warm the engine + module cache so the first measured iteration does
# not pay one-shot init cost.
sandbox.eval("nil")

runner.case_with_usage("6a-single-yield", sandbox) do
  sandbox.eval("Bench::YieldOnce.call(0) { |x| x }")
end

runner.case_with_usage("6b-block-no-yield", sandbox) do
  sandbox.eval("Bench::Ignore.call { 0 }")
end

runner.case_with_usage("6c-1000-yields-in-one-call", sandbox) do
  sandbox.eval("Bench::MapEach.call(Array.new(1000, 0)) { |x| x }")
end

runner.case_with_usage("6d-yield-break", sandbox) do
  sandbox.eval("Bench::EachBreak.call(Array.new(1000, 0)) { |_x| break }")
end

puts runner.write!
