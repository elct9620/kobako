# frozen_string_literal: true

# SPEC.md "Regression benchmarks" #2 — RPC round-trip latency.
# Detects regressions in the combined Wire codec, import function
# dispatch, and HandleTable lookup paths.
#
#   2a — Empty RPC: Service callable returns nil, guest invokes once
#   2b — Primitive arg: Integer arg returned verbatim
#   2c — Kwargs: Symbol-keyed kwargs (ext 0x00 on the wire)
#   2d — 1000 sequential RPCs inside one #run (per-RPC cost dominates
#        over #run setup/teardown)
#   2e — Handle chain (SPEC.md B-17): one Service returns a stateful
#        host object → guest holds it as a Handle → second RPC uses
#        the Handle as target. Exercises HandleTable#alloc on the
#        return path and HandleTable#fetch on the call path within a
#        single #run.
#
# Every case wraps one #run per iteration; the absolute number
# therefore includes a constant #run-overhead term (see #1 1b for
# its size). Regression detection is on the *delta* between cases,
# not on the absolute ips of any single case.

$LOAD_PATH.unshift File.expand_path("../lib", __dir__)
$LOAD_PATH.unshift File.expand_path("support", __dir__)

require "kobako"
require "runner"

runner = Kobako::Bench::Runner.new("rpc_roundtrip")

greeter = Class.new do
  def greet = "hi"
end

# memory_limit: nil — see benchmark/mruby_eval.rb. Long benchmark-ips
# loops must not trip the default 5 MiB per-run cap; the cap path is
# tested separately, this suite measures RPC throughput.
sandbox = Kobako::Sandbox.new(memory_limit: nil)
sandbox.define(:Bench)
       .bind(:Noop,    ->        {})
       .bind(:Echo,    ->(x)     { x })
       .bind(:Greet,   ->(name:) { name })
       .bind(:Factory, ->(_)     { greeter.new })

# Warm the engine + module cache so the first measured iteration
# does not pay one-shot init cost.
sandbox.run("nil")

runner.case("2a-empty-rpc") do
  sandbox.run("Bench::Noop.call")
end

runner.case("2b-primitive-arg") do
  sandbox.run("Bench::Echo.call(42)")
end

runner.case("2c-kwargs") do
  sandbox.run('Bench::Greet.call(name: "alice")')
end

runner.case("2d-1000-rpcs-in-one-run") do
  sandbox.run("1000.times { Bench::Noop.call }")
end

runner.case("2e-handle-chain") do
  sandbox.run("g = Bench::Factory.call(nil); g.greet")
end

puts runner.write!
