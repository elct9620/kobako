# frozen_string_literal: true

require "test_helper"
require "stringio"

# GC-safety regression coverage for the host-side dispatch Proc.
#
# Kobako::Sandbox installs a dispatch Proc on Kobako::Runtime
# (docs/behavior/dispatch.md B-12). The native ext holds that Proc across the whole
# guest invocation so that guest->host dispatch (B-13) and the yield
# round-trip (B-24) can call it. Because the Proc is reachable only from
# the ext while the guest runs, the ext is responsible for keeping it
# rooted against Ruby's garbage collector for the duration.
#
# Two GC mechanisms can break a Proc the ext fails to root, and they
# surface differently — these tests pin both so a future regression
# cannot reintroduce either silently:
#
#   * sweep (collection): the Proc is freed mid-invocation; the next
#     dispatch calls a dangling VALUE and the process SIGSEGVs.
#   * compaction (movement): the Proc is relocated; a cached raw VALUE
#     in the ext now points at the wrong object, so the dispatch call
#     lands on an unrelated receiver and the invocation fails as a
#     Kobako::SandboxError instead of round-tripping.
#
# GC.stress forces a full GC on every allocation, so a single
# dispatch-heavy invocation already triggers the sweep path
# deterministically; GC.compact between Proc binding and use forces the
# movement path. Both are restored in teardown so the global GC knobs do
# not leak into the rest of the suite.
class TestDispatchGcSafety < Minitest::Test
  def setup
    skip "native ext not compiled (run `bundle exec rake compile`)" unless defined?(Kobako::Runtime)
  end

  def teardown
    GC.stress = false
    GC.auto_compact = false if GC.respond_to?(:auto_compact=)
  end

  # Witness: under GC.stress the dispatch Proc is collected the moment
  # control leaves Sandbox#initialize unless the ext roots it. The
  # Handle-proxy #run path (B-34 / B-17) is the densest dispatch
  # round-trip available, so it is the surest trigger for the sweep
  # path. Pre-fix this SIGSEGVs on the first invocation.
  def test_handle_proxy_run_under_gc_stress_must_not_collect_dispatch_proc
    GC.stress = true

    3.times do
      sandbox = Kobako::Sandbox.new
      sandbox.preload(code: "Echo = ->(body) { body.read.upcase }", name: :Echo)

      assert_equal "HELLO WORLD", sandbox.run(:Echo, StringIO.new("hello world")),
                   "a Handle-proxy #run under GC.stress must round-trip the " \
                   "dispatched call without the dispatch Proc being garbage collected"
    end
  end

  # Witness: GC.compact moves live objects. The ext caches the dispatch
  # Proc as a raw VALUE, so a fix that only keeps the Proc alive (e.g. a
  # Ruby-side reference) but lets it move leaves the ext pointing at the
  # relocated-from slot. The break-in-block Service yield (B-25) exercises
  # the dispatch + yield round-trip; pre-fix this surfaces as a
  # Kobako::SandboxError ("transport dispatch Proc raised").
  def test_break_in_block_under_gc_compaction_must_keep_dispatch_proc_pinned
    skip "GC.compact unavailable on this Ruby build" unless GC.respond_to?(:compact)

    GC.auto_compact = true

    3.times do
      sandbox = Kobako::Sandbox.new
      sandbox.define(:Probe).bind(:Each, ->(items, &blk) { items.each(&blk) })

      # Force a compaction between Proc binding (in #initialize) and the
      # dispatch that uses it, so a moved-but-not-updated Proc is caught.
      GC.compact

      assert_equal :stop, sandbox.eval("Probe::Each.call([1, 2, 3]) { |x| break :stop if x == 2 }"),
                   "a break-in-block Service yield under GC compaction must keep the " \
                   "dispatch Proc pinned so the dispatch round-trip reaches the right receiver"
    end
  end
end
