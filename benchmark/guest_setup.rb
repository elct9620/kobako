# frozen_string_literal: true

# Characterization benchmark #13 — what the guest pays per unit of
# per-invocation setup input, measured against the real Guest Binary.
#
# #12 measures the host half of an invocation against a null guest, which
# by construction excludes everything below. #4 measures VM execution
# against a fixed script, so it never varies the input the compiler and
# the registry actually scale with. The two costs here therefore have no
# metric of their own even though both are paid on every invocation and
# both grow with something a Host App controls.
#
#   13a — mruby compile cost as source grows. Each waypoint is N `nil`
#         statements on a reused Sandbox, so the slope between waypoints
#         is per-statement parse + codegen. Statements rather than bytes:
#         a statement guarded by `if false` costs the same as one that
#         runs, so the cost tracks statement count, not source length or
#         work done. A regression that makes compilation super-linear in
#         statement count shows as growth in the slope.
#
#   13b — bound-Service materialisation on the guest, per bind path.
#         Every invocation re-materialises the whole registry into its
#         fresh mrb_state, so this is per-path-per-invocation, not
#         setup-time. Three shapes at a fixed count isolate what the path
#         itself costs: a top-level name pays for the leaf alone, a shared
#         namespace amortises one prefix resolution across the group, and
#         a per-path namespace pays for its own. The bindings are declared
#         but never referenced, so the figure is materialisation with no
#         dispatch folded in. #12's `12d` sees only the host's share of
#         this — the preamble it encodes — so the guest's is unmetered
#         without these rows.

$LOAD_PATH.unshift File.expand_path("../lib", __dir__)
$LOAD_PATH.unshift File.expand_path("support", __dir__)

require "kobako"
require "guest"
require "runner"

runner = Kobako::Bench::Runner.new("guest_setup")

# Resolved once outside every measured block so the KOBAKO_BENCH_WASM
# lookup never lands in the timer.
guest = Kobako::Bench::Guest.path

# memory_limit: nil — see benchmark/mruby_eval.rb for rationale.
def warm_sandbox(guest, paths: [])
  sandbox = Kobako::Sandbox.new(wasm_path: guest, memory_limit: nil)
  paths.each { |path| sandbox.bind(path, -> { 1 }) }
  sandbox.eval("nil")
  sandbox
end

# 13a — compile cost as statement count grows. The sources are built at
# suite setup so no string construction lands inside the timer, mirroring
# mruby_eval.rb's ARITH_SCRIPT et al.
STATEMENT_WAYPOINTS = [1, 20, 100, 400].freeze
STATEMENT_SOURCES = STATEMENT_WAYPOINTS.to_h { |n| [n, (["nil"] * n).join("\n")] }.freeze

compile_sandbox = warm_sandbox(guest)
STATEMENT_WAYPOINTS.each do |n|
  source = STATEMENT_SOURCES.fetch(n)
  runner.case_with_usage("13a-eval-statements-#{n}") { compile_sandbox.eval(source) }
end

# 13b — guest-side binding materialisation. The count is fixed so the
# shapes are directly comparable; the zero-binding row is the floor all
# three subtract, and is shape-independent because there is no path to
# resolve. `nil` as the script keeps the invocation's own work out of it.
BINDING_COUNT = 32
BINDING_SHAPES = {
  "top" => ->(i) { "Svc#{i}" },
  "shared-namespace" => ->(i) { "Bench::Svc#{i}" },
  "own-namespace" => ->(i) { "Bench#{i}::Svc#{i}" }
}.freeze

bare_sandbox = warm_sandbox(guest)
runner.case_with_usage("13b-eval-bindings-0") { bare_sandbox.eval("nil") }

BINDING_SHAPES.each do |shape, path_for|
  paths = Array.new(BINDING_COUNT) { |i| path_for.call(i) }
  sandbox = warm_sandbox(guest, paths: paths)
  runner.case_with_usage("13b-eval-bindings-#{BINDING_COUNT}-#{shape}") { sandbox.eval("nil") }
end

puts runner.write!
