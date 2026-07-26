# frozen_string_literal: true

# Characterization benchmark #12 — the host's per-invocation cost, read
# as a total against a guest that does no work.
#
# Every other sandbox-driven suite reports a total that bundles host and
# guest, and gates on `wall_time` — the guest export alone. That leaves
# the host half of an invocation with no metric of its own: the
# Context and its Catalog::Handles, the Run envelope, the frame stream,
# the boundary crossing, the Outcome decode, and the frozen Execution.
# The one derived stand-in, `1/ips - wall_time`, is a difference of two
# near-equal numbers, and on the thousand-call rows it loses every
# significant digit — a single round read +326% on `2d` and -94% on the
# neighbouring `2f`.
#
# Driving the null-guest fixture instead makes the total the measurement:
# the guest ignores its input and answers a constant nil Result, so what
# is left is what the host does. The cost of a guest doing real work is
# already the subject of #2, #4 and #6; this suite deliberately excludes
# it.
#
#   12a — #eval: the floor every invocation pays.
#   12b — #run with no arguments: adds the Run envelope over 12a.
#   12c — #run with positional and keyword arguments: adds the payload
#         codec's argument encoding over 12b.
#   12d — the same #eval on a Sandbox carrying bound Services: the HOST
#         half of what a registry costs each invocation, which is the
#         preamble it encodes. The guest half — materializing each
#         binding into the mrb_state — is out of frame here by
#         construction, which is the point: the two were previously
#         inseparable.

$LOAD_PATH.unshift File.expand_path("../lib", __dir__)
$LOAD_PATH.unshift File.expand_path("support", __dir__)

require "kobako"
require "paths"
require "runner"

runner = Kobako::Bench::Runner.new("host_invocation")

# The fixture is committed rather than built, so this suite runs wherever
# the ext does — it never reads data/kobako.wasm and ignores
# KOBAKO_BENCH_WASM, which would defeat the isolation.
NULL_GUEST = Kobako::Bench::Paths.fixture("minimal_null_guest.wat")

sandbox = Kobako::Sandbox.new(wasm_path: NULL_GUEST)
sandbox.eval("warm")

runner.case("12a-eval") { sandbox.eval("nil") }
runner.case("12b-run-no-args") { sandbox.run(:Noop) }
runner.case("12c-run-args") { sandbox.run(:Echo, 42, name: :alice) }

# The registry is re-sent every invocation, so a growing preamble is a
# per-invocation host cost even when the guest calls into none of it.
bound = Kobako::Sandbox.new(wasm_path: NULL_GUEST)
8.times { |i| bound.bind("Bench::S#{i}", -> {}) }
bound.eval("warm")

runner.case("12d-eval-8-bound-services") { bound.eval("nil") }

puts runner.write!
