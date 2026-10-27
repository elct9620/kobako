# frozen_string_literal: true

# Characterization benchmark #14 — what a Mount's block costs per open.
# A block given to Sandbox#mount is asked once per open or change, and
# each ask re-enters Ruby from the guest span through the GVL, so its
# cost grows with the opens a script makes and has no other metric.
#
#   14a / 14b — one File.read beneath a Tmpfs Mount, without and with a
#               block. The delta is one block call plus the
#               per-invocation Grant translation.
#   14c / 14d — 100 File.read calls in one invocation, without and with a
#               block. The delta divided by 100 is the per-open block
#               cost once the per-invocation setup is amortized.
#
# Every case wraps one #eval per iteration; read the deltas, not the
# absolute ips of any single case.

$LOAD_PATH.unshift File.expand_path("../lib", __dir__)
$LOAD_PATH.unshift File.expand_path("support", __dir__)

require "kobako"
require "guest"
require "runner"

runner = Kobako::Bench::Runner.new("mount")

guest = Kobako::Bench::Guest.path

# memory_limit: nil — see benchmark/mruby_eval.rb for rationale.
def mounted_sandbox(guest, &policy)
  sandbox = Kobako::Sandbox.new(wasm_path: guest, memory_limit: nil)
  sandbox.mount("/data", Kobako::Filesystem::Tmpfs.new("note.txt" => "hello"), &policy)
  sandbox.eval("nil")
  sandbox
end

READ_ONCE = 'File.read("/data/note.txt")'
READ_100 = '100.times { File.read("/data/note.txt") }'

plain = mounted_sandbox(guest)
guarded = mounted_sandbox(guest) { |_path, _access| true }

runner.case_with_usage("14a-read-1") { plain.eval(READ_ONCE) }
runner.case_with_usage("14b-read-1-block") { guarded.eval(READ_ONCE) }
runner.case_with_usage("14c-read-100") { plain.eval(READ_100) }
runner.case_with_usage("14d-read-100-block") { guarded.eval(READ_100) }

puts runner.write!
