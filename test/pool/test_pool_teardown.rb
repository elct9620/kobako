# frozen_string_literal: true

require "test_helper"
require "weakref"

# Coverage for Kobako::Pool teardown — reachability is the lifecycle
# (docs/behavior/runtime.md B-48): dropping the last Pool reference
# releases the Pool and its pooled Sandboxes through ordinary garbage
# collection, and an in-flight #with holder stays valid until its block
# exits. Drives the real data/kobako.wasm.
class TestPoolTeardown < Minitest::Test
  include E2eGuestHelper

  # CRuby's conservative stack scan can pin a dropped reference for a
  # few cycles, so reclamation is asserted through a bounded retry
  # rather than a single GC pass.
  GC_PASSES = 10

  # B-48: the Pool has no teardown verb — once the Host App drops its
  # last reference, the pooled Sandboxes go with it. A regression
  # parking Sandboxes in any pool-external strong reference (a process
  # registry, a finalizer, a watcher thread) keeps these WeakRefs alive
  # past every GC pass and fails here.
  def test_dropping_the_last_pool_reference_releases_pool_and_sandboxes
    refs = pool_and_sandbox_refs
    assert reclaimed?(refs),
           "an unreachable Pool and its pooled Sandboxes must be reclaimed by garbage collection (B-48)"
  end

  # B-48: a Sandbox held by an in-flight #with block remains valid until
  # the block exits, even after the holder drops its own Pool reference
  # mid-block.
  def test_in_flight_checkout_stays_valid_after_pool_reference_dropped
    pool = Kobako::Pool.new(slots: 1)
    result = pool.with do |sandbox|
      pool = nil
      GC.start
      sandbox.eval("40 + 2")
    end
    assert_nil pool
    assert_equal 42, result,
                 "a Sandbox inside an in-flight #with must stay invocable after its Pool reference is dropped (B-48)"
  end

  private

  # Construct a Pool and check one Sandbox out and back in inside a
  # short-lived Thread, handing the caller only WeakRefs. The dead
  # thread's stacks are exempt from the conservative scan, so no stale
  # slot from the checkout call chain can pin either object.
  def pool_and_sandbox_refs
    Thread.new do
      pool = Kobako::Pool.new(slots: 1)
      sandbox_ref = pool.with { |sandbox| WeakRef.new(sandbox) }
      [WeakRef.new(pool), sandbox_ref]
    end.value
  end

  def reclaimed?(refs)
    GC_PASSES.times do
      GC.start
      return true if refs.none?(&:weakref_alive?)
    end
    false
  end
end
