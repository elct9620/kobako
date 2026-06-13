# frozen_string_literal: true

# Characterization benchmark (not in SPEC.md release gate) — covers
# the Sandbox#preload + Sandbox#run path that did not exist when the
# original SPEC #1..#5 gated suite was written. The other benchmarks all measure
# inside Sandbox#eval; this file isolates the cost dimensions the
# two new verbs add.
#
# Positioning: #preload and #run are independent features. They are
# NOT a "faster #eval"; the joint flow (#preload(code:, name:) +
# #run(target)) is the setup-once / dispatch-many use case from SPEC
# J-06, and #preload may also be combined with #eval to share helper
# code across one-shot scripts. The cases below isolate each new
# verb's contribution.
#
#   9a — Preload registration cost (host-side trial-compile path).
#        Steady-state cost of `Sandbox.new + N #preload(code:,
#        name:)`. Each #preload trial-compiles the source against a
#        fresh host mrb_state to catch a compile error early,
#        so registration is non-trivial. The 1 / 8 / 64 waypoints
#        characterize linearity — a regression that adds per-snippet
#        O(N) work would show as super-linear growth in the delta
#        between waypoints. The Sandbox.new term is constant across
#        waypoints; subtract a Sandbox.new baseline (cold_start 1a)
#        to isolate per-snippet cost.
#
#   9b — Run dispatch baseline. Warm Sandbox with one preloaded
#        snippet defining `Noop`; `sandbox.run(:Noop)` cost in
#        steady state. Isolates the #run-specific entry path: host
#        pre-flight (Invocation envelope construction, target / args
#        / kwargs validation) plus
#        guest-side constant resolution.
#
#   9c — Run dispatch with one positional Integer arg.
#        `sandbox.run(:Echo, 42)` exercises the Invocation envelope's
#        args Array encoding.
#
#   9d — Run dispatch with Symbol-keyed kwargs. `sandbox.run(:Greet,
#        name: :alice)` puts a Symbol key through the Invocation
#        envelope's kwargs Map. The ext 0x00 codec path here is the
#        host→guest direction — structurally distinct from the
#        guest→host Transport kwargs path measured by transport_roundtrip 2c,
#        even though both rely on the same Symbol wire ext.
#
#   9e — Per-invocation snippet replay overhead. Same #run(:Noop)
#        dispatch, but with 0 / 8 / 64 additional helper snippets
#        preloaded alongside `Noop`. Every snippet is replayed
#        against the fresh mrb_state on every invocation,
#        so the slope between waypoints characterizes per-snippet
#        replay cost on the steady-state #run path. A regression
#        that makes replay super-linear in snippet count would show
#        here.
#
#   9f — #run dispatch with a non-wire-representable positional
#        arg via host→guest auto-wrap. The args walker routes
#        the StringIO through Codec::Utils.deep_wrap, which routes
#        non-wire-representable leaves through Catalog::Handles#alloc;
#        the guest receives a +Kobako::Handle+ proxy. The entrypoint
#        ignores the proxy, so this case isolates the host-side
#        auto-wrap cost (predicate + alloc + wire encode) without
#        also incurring a guest→host Transport roundtrip — 9c / 9d only
#        cover the wire-fast path and miss any regression in the
#        auto-wrap branch.
#
$LOAD_PATH.unshift File.expand_path("../lib", __dir__)
$LOAD_PATH.unshift File.expand_path("support", __dir__)

require "stringio"

require "kobako"
require "runner"

runner = Kobako::Bench::Runner.new("preload_dispatch")

NOOP_SNIPPET_CODE = <<~RUBY
  module Noop
    def self.call
      nil
    end
  end
RUBY

ECHO_SNIPPET_CODE = <<~RUBY
  module Echo
    def self.call(x)
      x
    end
  end
RUBY

# Entrypoints accept kwargs as a trailing Hash argument. `name:` becomes
# opts[:name] inside the call.
GREET_SNIPPET_CODE = <<~RUBY
  module Greet
    def self.call(opts)
      opts[:name]
    end
  end
RUBY

# 9f auto-wrap target. The entrypoint discards the Handle proxy so
# the case measures only the host-side wrap path (predicate +
# Catalog::Handles#alloc + wire encode) — calling #read on the proxy
# would add a guest→host Transport roundtrip that confounds the signal.
WRAP_SNIPPET_CODE = <<~RUBY
  module Wrap
    def self.call(_handle)
      nil
    end
  end
RUBY

# Small but realistic helper snippet shape: module + constant +
# self-method. Each snippet is ~70 bytes of source, so the 9e-64
# waypoint replays ~4.5 KiB of helper source against the fresh
# mrb_state on every invocation — representative of a "small set
# of helper modules" deployment, not a degenerate empty constant.
def helper_snippet_code(index)
  <<~RUBY
    module Helper#{index}
      VAL = #{index}
      def self.value
        VAL
      end
    end
  RUBY
end

def helper_snippet_name(index)
  :"Helper#{index}"
end

# Pre-compute the helper snippet sources and names ONCE at suite
# setup time. The 9a / 9e timed blocks below index into these frozen
# arrays so the heredoc interpolation and Symbol construction cost
# stays out of the measurement window — only Sandbox.new, #preload,
# and #run land inside the timer. Mirrors the mruby_eval.rb pattern
# of declaring ARITH_SCRIPT etc. at module top rather than building
# strings inside the runner.case block.
HELPER_MAX = 64
HELPER_CODES = Array.new(HELPER_MAX) { |i| helper_snippet_code(i) }.freeze
HELPER_NAMES = Array.new(HELPER_MAX) { |i| helper_snippet_name(i) }.freeze

# Process-wide warm-up so 9a's first iteration does not pay the
# first-Sandbox cold cost (Engine init + Module JIT compile).
# Mirrors the warm-up pattern in transport_roundtrip / codec / mruby_eval.
Kobako::Sandbox.new.eval("nil")

# 9a — preload registration cost. Each iteration constructs a fresh
# Sandbox and registers N helper snippets via index lookups into the
# pre-computed +HELPER_CODES+ / +HELPER_NAMES+ arrays — no string or
# Symbol construction inside the timer. The Sandbox.new term is
# constant across the three waypoints; subtract cold_start 1a
# (Sandbox.new alone) to recover the per-snippet preload cost.
# memory_limit: nil — see benchmark/mruby_eval.rb for rationale.
[1, 8, 64].each do |n|
  runner.case("9a-sandbox-new+preload-#{n}-source") do
    sandbox = Kobako::Sandbox.new(memory_limit: nil)
    n.times { |i| sandbox.preload(code: HELPER_CODES[i], name: HELPER_NAMES[i]) }
  end
end

# Shared dispatch sandbox for 9b / 9c / 9d. One warm-up #run seals
# the Service / snippet tables so the first measured
# iteration does not pay seal cost.
dispatch_sandbox = Kobako::Sandbox.new(memory_limit: nil)
dispatch_sandbox.preload(code: NOOP_SNIPPET_CODE, name: :Noop)
dispatch_sandbox.preload(code: ECHO_SNIPPET_CODE, name: :Echo)
dispatch_sandbox.preload(code: GREET_SNIPPET_CODE, name: :Greet)
dispatch_sandbox.run(:Noop) # warm + seal

runner.case_with_usage("9b-run-dispatch-empty", dispatch_sandbox) { dispatch_sandbox.run(:Noop) }
runner.case_with_usage("9c-run-dispatch-positional", dispatch_sandbox) { dispatch_sandbox.run(:Echo, 42) }
runner.case_with_usage("9d-run-dispatch-kwargs", dispatch_sandbox) { dispatch_sandbox.run(:Greet, name: :alice) }

# 9e — per-invocation snippet replay overhead. Each waypoint owns a
# Sandbox with N additional helpers preloaded alongside the Noop
# entrypoint. The slope between 0 / 8 / 64 isolates per-snippet
# replay cost on the steady-state #run path. (9e's preload calls
# already sit outside the timer, but the +HELPER_CODES+ /
# +HELPER_NAMES+ lookup keeps the setup code uniform with 9a.)
[0, 8, 64].each do |n|
  sandbox = Kobako::Sandbox.new(memory_limit: nil)
  sandbox.preload(code: NOOP_SNIPPET_CODE, name: :Noop)
  n.times { |i| sandbox.preload(code: HELPER_CODES[i], name: HELPER_NAMES[i]) }
  sandbox.run(:Noop) # warm + seal
  runner.case_with_usage("9e-run-replay-#{n}-snippets", sandbox) { sandbox.run(:Noop) }
end

# 9f — auto-wrap path. Dedicated sandbox because dispatch_sandbox is
# already sealed by 9b's warm-up #run; subsequent #preload would
# raise (preload after seal is rejected). The same +autowrap_arg+ is
# reused across iterations, but the per-invocation reset clears the
# Catalog::Handles at the start of every
# invocation, so each measured #run still pays for one fresh
# Catalog::Handles#alloc.
autowrap_sandbox = Kobako::Sandbox.new(memory_limit: nil)
autowrap_sandbox.preload(code: WRAP_SNIPPET_CODE, name: :Wrap)
autowrap_arg = StringIO.new("payload")
autowrap_sandbox.run(:Wrap, autowrap_arg) # warm + seal

runner.case_with_usage("9f-run-dispatch-autowrap", autowrap_sandbox) do
  autowrap_sandbox.run(:Wrap, autowrap_arg)
end

puts runner.write!
