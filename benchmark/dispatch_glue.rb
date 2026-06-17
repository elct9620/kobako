# frozen_string_literal: true

# Characterization benchmark #10 (not in SPEC.md release gate) —
# isolates the GVL-held host glue of a single guest->host dispatch: the
# work the +Runtime#on_dispatch+ Proc performs on the Ruby side (decode
# Request -> resolve target -> invoke Service -> encode Response). It
# calls +Kobako::Transport::Dispatcher.dispatch+ directly with
# pre-encoded Request bytes, so NO wasm, boundary crossing, or
# guest-side codec is in the measurement window.
#
# This is the predictive half of the GVL-impact toolkit; #7
# (concurrent/threads.rb) is the confirmation half. A No-GVL design
# (release the GVL during wasm compute, re-acquire via with_gvl only to
# run the dispatch Proc) would parallelize everything EXCEPT this glue,
# so this glue time `G` is the only GVL-held, non-parallelizable
# fraction of a dispatch. The multi-core speedup ceiling for a workload
# that performs k dispatches in an invocation of total wall-time T is
# bounded by Amdahl with serial fraction d = k*G / T. Compose G here
# with the full roundtrip (transport_roundtrip 2d) for the per-dispatch
# floor of d, and confirm the realized speedup with #7 once nogvl
# exists. This suite measures G per Service shape; T and k are
# workload-determined (set by the Host App's bound Services), so the gem
# publishes G and the method, never a single d.
#
# The bound Services are pure-CPU on purpose: a Service doing real I/O
# releases the GVL during the syscall, so its wait already overlaps
# across threads today and must NOT count toward G. G is exactly the
# Ruby-CPU glue — codec + resolution + the Service's own CPU.
#
#   10a — empty call: Service returns nil. Floor: decode 5-field
#         envelope + path lookup + invoke + encode nil Response.
#   10b — primitive arg: one Integer arg decoded and returned verbatim.
#   10c — kwargs: Symbol-keyed kwargs (ext 0x00) decoded into the call.
#   10d — small structured return: Service returns a 16-element Integer
#         Array. Isolates the Response-encode growth a data-returning
#         Binding pays over 10a's nil return.
#   10e — larger structured return: 256-element Array. The 10d->10e
#         slope characterizes how G scales with returned-payload size —
#         the dominant term for a "fetch a row, hand it back" Binding.
#
# Read alongside transport_roundtrip 2a..2d (the FULL roundtrip,
# G + guest codec + boundary) to recover the parallelizable remainder
# per dispatch as (2x per-call) - G.

$LOAD_PATH.unshift File.expand_path("../lib", __dir__)
$LOAD_PATH.unshift File.expand_path("support", __dir__)

require "kobako"
require "runner"

runner = Kobako::Bench::Runner.new("dispatch_glue")

# Per-Sandbox registry + Handle table, wired exactly as
# Kobako::Sandbox#initialize wires them into the on_dispatch Proc.
handler = Kobako::Catalog::Handles.new
namespaces = Kobako::Catalog::Namespaces.new(handler: handler)
namespaces.define(:Bench)
          .bind(:Noop,   ->        {})
          .bind(:Echo,   ->(x)     { x })
          .bind(:Greet,  ->(name:) { name })
          .bind(:Small,  ->        { Array.new(16) { |i| i } })
          .bind(:Large,  ->        { Array.new(256) { |i| i } })

# block_given is false on every case below, so yield_to_guest is never
# invoked; a raising stub localises any accidental block path.
yield_to_guest = ->(_bytes) { raise "yield_to_guest must not fire in dispatch_glue" }

# Pre-encode Request bytes ONCE so msgpack envelope construction stays
# out of the measurement window — only Dispatcher.dispatch is timed.
def request_bytes(target, method_name, args: [], kwargs: {})
  Kobako::Transport::Request.new(
    target: "Bench::#{target}", method_name: method_name, args: args, kwargs: kwargs
  ).encode
end

NOOP_REQ  = request_bytes(:Noop, "call")
ECHO_REQ  = request_bytes(:Echo, "call", args: [42])
GREET_REQ = request_bytes(:Greet, "call", kwargs: { name: :alice })
SMALL_REQ = request_bytes(:Small, "call")
LARGE_REQ = request_bytes(:Large, "call")

# Warm process-wide codec / inline caches so the first measured case
# does not pay cold-cache cost. Mirrors the warm-up in the other suites.
Kobako::Transport::Dispatcher.dispatch(NOOP_REQ, namespaces, handler, yield_to_guest)

runner.case("10a-empty-call") do
  Kobako::Transport::Dispatcher.dispatch(NOOP_REQ, namespaces, handler, yield_to_guest)
end

runner.case("10b-primitive-arg") do
  Kobako::Transport::Dispatcher.dispatch(ECHO_REQ, namespaces, handler, yield_to_guest)
end

runner.case("10c-kwargs") do
  Kobako::Transport::Dispatcher.dispatch(GREET_REQ, namespaces, handler, yield_to_guest)
end

runner.case("10d-small-return-16") do
  Kobako::Transport::Dispatcher.dispatch(SMALL_REQ, namespaces, handler, yield_to_guest)
end

runner.case("10e-large-return-256") do
  Kobako::Transport::Dispatcher.dispatch(LARGE_REQ, namespaces, handler, yield_to_guest)
end

puts runner.write!
