# frozen_string_literal: true

require "test_helper"

# E2E — a guest block run from a fiber other than the one driving its
# invocation. The guest's Store and the value budget belong to the driving
# fiber, so the block must refuse before it re-enters the guest; the guest
# reports whether its block body ever ran.
class TestE2EYieldForeignFiber < Minitest::Test
  include E2eGuestHelper

  GUEST = <<~RUBY
    ran = false
    refusal = Probe::Runs.call { ran = true }
    [refusal, ran]
  RUBY

  # @behavior T-290
  def test_a_block_run_on_another_thread_refuses
    %i[hold release].each do |gvl|
      sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM, gvl: gvl)
      sandbox.bind("Probe::Runs", ->(&block) { refusal { Thread.new { block.call }.value } })

      assert_equal ["ThreadError", false], sandbox.eval(GUEST).value,
                   "a guest block a Service runs on its own Thread through Sandbox#eval (gvl: #{gvl}) " \
                   "must raise ThreadError in the Service without running the block"
    end
  end

  # @behavior T-291
  def test_a_block_run_inside_an_external_enumerator_refuses
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("Probe::Runs", ->(&block) { refusal { Enumerator.new { |y| y << block.call }.next } })

    assert_equal ["ThreadError", false], sandbox.eval(GUEST).value,
                 "a guest block a Service runs through Enumerator#next via Sandbox#eval " \
                 "must raise ThreadError in the Service without running the block"
  end

  private

  def refusal
    yield
    nil
  rescue ThreadError => e
    e.class.name
  end
end
