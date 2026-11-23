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
#   14e — 100 File.read calls of one file 8 directories deep beneath a
#         Bind. Every walk finds its directories kept, so the row prices
#         the cache's lock on a hit.
#   14f — one File.read in each of 100 directories beneath one Bind,
#         more than the 64 it keeps, so each round walks again.
#   14g — one File.read in each of 32 directories beneath each of 4
#         Binds, 128 in all against the 64 the process keeps.
#
# Every case wraps one #eval per iteration; read the deltas, not the
# absolute ips of any single case.

$LOAD_PATH.unshift File.expand_path("../lib", __dir__)
$LOAD_PATH.unshift File.expand_path("support", __dir__)

require "fileutils"
require "kobako"
require "tmpdir"
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

# A host tree of `count` directories, each holding note.txt, `depth` deep.
def host_tree(root, count, depth)
  count.times do |at|
    dir = File.join(root, "d#{at}", *Array.new(depth - 1) { |level| "l#{level}" })
    FileUtils.mkdir_p(dir)
    File.write(File.join(dir, "note.txt"), "hello")
  end
end

def bound_sandbox(guest, roots)
  sandbox = Kobako::Sandbox.new(wasm_path: guest, memory_limit: nil)
  roots.each_with_index { |root, at| sandbox.mount("/b#{at}", Kobako::Filesystem::Bind.new(root)) }
  sandbox.eval("nil")
  sandbox
end

hosts = Dir.mktmpdir("kobako-bench-mount")
deep, wide, *spread = %w[deep wide s0 s1 s2 s3].map { |name| File.join(hosts, name) }
host_tree(deep, 1, 8)
host_tree(wide, 100, 1)
spread.each { |root| host_tree(root, 32, 1) }

DEEP_PATH = "/b0/d0/#{(0..6).map { |level| "l#{level}" }.join("/")}/note.txt".freeze
READ_DEEP = "100.times { File.read(#{DEEP_PATH.inspect}) }".freeze
READ_WIDE = "100.times { |at| File.read(\"/b0/d\#{at}/note.txt\") }"
READ_SPREAD = "4.times { |b| 32.times { |at| File.read(\"/b\#{b}/d\#{at}/note.txt\") } }"

plain = mounted_sandbox(guest)
guarded = mounted_sandbox(guest) { |_path, _access| true }

runner.case_with_usage("14a-read-1") { plain.eval(READ_ONCE) }
runner.case_with_usage("14b-read-1-block") { guarded.eval(READ_ONCE) }
runner.case_with_usage("14c-read-100") { plain.eval(READ_100) }
runner.case_with_usage("14d-read-100-block") { guarded.eval(READ_100) }

deep_bound = bound_sandbox(guest, [deep])
wide_bound = bound_sandbox(guest, [wide])
spread_bound = bound_sandbox(guest, spread)
# A host directory a Bind holds open cannot be removed on Windows, so the
# Sandboxes go first.
at_exit do
  deep_bound = wide_bound = spread_bound = nil
  GC.start
  FileUtils.remove_entry(hosts)
end

runner.case_with_usage("14e-bind-read-100-deep") { deep_bound.eval(READ_DEEP) }
runner.case_with_usage("14f-bind-read-100-dirs") { wide_bound.eval(READ_WIDE) }
runner.case_with_usage("14g-bind-read-4x32-dirs") { spread_bound.eval(READ_SPREAD) }

puts runner.write
