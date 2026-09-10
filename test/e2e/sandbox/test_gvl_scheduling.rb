# frozen_string_literal: true

require "test_helper"

# E2E (Layer 4) — the gvl: scheduling mode through real mruby.
# gvl: :release drops Ruby's GVL for the guest span so distinct Sandboxes
# on distinct Threads run in parallel; :hold keeps it. Releasing changes scheduling only, so every witness here
# runs the same scenario under both modes and asserts the observable
# outcome is identical — the guest value, a guest→host dispatch result, a
# nested dispatch result (which exercises the one re-acquire of the
# released GVL), and the stdout capture all hold across the modes. The
# host-parallel journey the feature exists for is walked end to end by
# running distinct :release Sandboxes on distinct Threads. Option
# validation and the gvl reader are pinned host-side in
# test_sandbox_options.rb.
class TestE2EGvlScheduling < Minitest::Test
  include E2eGuestHelper

  # @behavior RT-022
  def test_release_runs_a_plain_eval_identically_to_hold
    values = Kobako::SandboxOptions::GVL_MODES.map do |mode|
      Kobako::Sandbox.new(wasm_path: REAL_WASM, gvl: mode).eval("2 ** 10").value
    end

    assert_equal [1024, 1024], values,
                 "a plain eval through Sandbox.new(gvl:) must yield the same guest value under " \
                 ":hold and :release"
  end

  # @behavior RT-023
  def test_release_preserves_a_guest_to_host_dispatch_result
    values = Kobako::SandboxOptions::GVL_MODES.map do |mode|
      sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM, gvl: mode)
      sandbox.bind("Echo::Shout", ->(text) { "#{text}!" })
      sandbox.eval("Echo::Shout.call('gvl')").value
    end

    assert_equal ["gvl!", "gvl!"], values,
                 "a guest→host dispatch must return the same result under :hold and :release — " \
                 "release re-acquires the GVL for the callback"
  end

  # @behavior RT-024
  def test_release_preserves_a_nested_dispatch_result
    values = Kobako::SandboxOptions::GVL_MODES.map do |mode|
      sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM, gvl: mode)
      sandbox.bind("A::Step", ->(name:, &blk) { "A[#{name}]:#{blk.call}" })
      sandbox.bind("B::Fetch", ->(key) { "fetched:#{key}" })
      sandbox.eval("A::Step.call(name: 'outer') { B::Fetch.call('k1') }").value
    end

    assert_equal ["A[outer]:fetched:k1", "A[outer]:fetched:k1"], values,
                 "nested guest→host dispatch must resolve identically under :hold and :release — " \
                 "the released GVL is re-acquired once and held across the nested frames"
  end

  # @behavior RT-025
  def test_release_preserves_the_stdout_capture
    captures = Kobako::SandboxOptions::GVL_MODES.map do |mode|
      execution = Kobako::Sandbox.new(wasm_path: REAL_WASM, gvl: mode).eval("$stdout.write('hi from guest')")
      [execution.stdout, execution.stdout_truncated?]
    end

    assert_equal captures.first, captures.last,
                 "the stdout capture must be identical under :hold and :release — " \
                 "release changes scheduling only"
  end

  # @behavior RT-026
  # The journey the feature exists for: a Host App runs guest code on
  # distinct :release Sandboxes across distinct Threads and every thread
  # returns its own correct result, so releasing the GVL keeps each
  # invocation isolated while allowing them to run host-parallel.
  def test_release_runs_host_parallel_across_threads_with_isolated_results
    results = (0...4).map do |i|
      Thread.new do
        code = "#{i} * 100"
        Kobako::Sandbox.new(wasm_path: REAL_WASM, gvl: :release).eval(code).value
      end
    end.map(&:value)

    assert_equal [0, 100, 200, 300], results,
                 "distinct :release Sandboxes on distinct Threads must each return their own " \
                 "isolated result"
  end
end
