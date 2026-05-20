# frozen_string_literal: true

# SPEC.md "Regression benchmarks" #2 — RPC round-trip latency.
# Detects regressions in the combined Wire codec, import function
# dispatch, and HandleTable lookup paths.
#
#   2a — Empty RPC: Service callable returns nil, guest invokes once
#   2b — Primitive arg: Integer arg returned verbatim
#   2c — Kwargs: Symbol-keyed kwargs (ext 0x00 on the wire)
#   2d — 1000 sequential RPCs inside one #eval (per-RPC cost
#        dominates over per-invocation setup/teardown)
#   2e — Handle chain (SPEC.md B-17): one Service returns a stateful
#        host object → guest holds it as a Handle → second RPC uses
#        the Handle as target. Exercises HandleTable#alloc on the
#        return path and HandleTable#fetch on the call path within a
#        single invocation.
#
# Every case wraps one #eval per iteration; the absolute number
# therefore includes a constant per-invocation overhead term (see
# #1 1b for its size). Regression detection is on the *delta*
# between cases, not on the absolute ips of any single case.

$LOAD_PATH.unshift File.expand_path("../lib", __dir__)
$LOAD_PATH.unshift File.expand_path("support", __dir__)

require "kobako"
require "runner"

runner = Kobako::Bench::Runner.new("rpc_roundtrip")

greeter = Class.new do
  def greet = "hi"
end

# memory_limit: nil — see benchmark/mruby_eval.rb. The default 1 MiB
# per-invocation delta cap is enforced on its own dedicated path; this
# suite measures RPC throughput, so we keep the limiter callback out
# of the wasmtime hot loop.
sandbox = Kobako::Sandbox.new(memory_limit: nil)
sandbox.define(:Bench)
       .bind(:Noop,    ->        {})
       .bind(:Echo,    ->(x)     { x })
       .bind(:Greet,   ->(name:) { name })
       .bind(:Factory, ->(_)     { greeter.new })

# Warm the engine + module cache so the first measured iteration
# does not pay one-shot init cost.
sandbox.eval("nil")

runner.case("2a-empty-rpc") do
  sandbox.eval("Bench::Noop.call")
end

runner.case("2b-primitive-arg") do
  sandbox.eval("Bench::Echo.call(42)")
end

runner.case("2c-kwargs") do
  sandbox.eval('Bench::Greet.call(name: "alice")')
end

runner.case("2d-1000-rpcs-in-one-eval") do
  sandbox.eval("1000.times { Bench::Noop.call }")
end

runner.case("2e-handle-chain") do
  sandbox.eval("g = Bench::Factory.call(nil); g.greet")
end

puts runner.write!
