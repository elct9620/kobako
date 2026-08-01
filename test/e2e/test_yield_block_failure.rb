# frozen_string_literal: true

require "test_helper"

# E2E (Layer 4) — what happens to the exception a guest block raises
# (docs/behavior/yield.md B-24). The block runs inside the guest, so the
# failure is the guest's own: the Service gets a chance to rescue it at
# its yield site, and if it does not, the exception continues in the frame
# that raised it rather than being rebuilt as a Service failure. The
# round-trip itself lives in test_yield.rb, the break / return unwind
# discrimination in test_yield_unwind.rb.
class TestE2EYieldBlockFailure < Minitest::Test
  include E2eGuestHelper

  # B-24: the block's exception reaches the Service's yield site, where it
  # may be rescued. Unrescued, it is still the guest's own failure — the
  # same exception object continuing in the frame that raised it — so it
  # attributes to the sandbox (E-04) rather than to the Service.
  def test_b24_block_raise_surfaces_to_service_yield_site
    err = assert_raises(Kobako::SandboxError) do
      yielding_sandbox.eval('Probe::Boom.call { raise "from guest block" }')
    end

    assert_equal "RuntimeError", err.klass,
                 "a block raise left unrescued by the Service must reach the Host App under " \
                 "the class the guest raised, not the class a failed Service call raises"
    assert_equal "from guest block", err.message,
                 "the guest's own message must arrive as it was raised, unprefixed by the " \
                 "classes it passed through"
  end

  # The guest keeps a reference to what it raises, so its rescue can ask
  # whether what came back is that object rather than one like it.
  IDENTITY_PROBE = <<~RUBY
    mine = ArgumentError.new('carried')
    begin
      Probe::Boom.call { raise mine }
    rescue ArgumentError => e
      e.equal?(mine) ? 'same object' : 'a different object'
    end
  RUBY

  def test_b24_unrescued_block_raise_is_rescuable_in_the_guest_as_itself
    seen = yielding_sandbox.eval(IDENTITY_PROBE).value

    assert_equal "same object", seen,
                 "a block raise the Service left unrescued must continue in the guest as the " \
                 "very object it raised, so a rescue reads the class and every field it set"
  end

  def test_b24_a_service_that_rescues_the_blocks_raise_reports_its_own_failure
    err = assert_raises(Kobako::ServiceError) do
      rescuing_sandbox(raising: true).eval('Probe::Swallow.call { raise "from guest block" }')
    end

    assert_match(/IOError: the Service failed on its own/, err.message,
                 "a Service that rescued the block's failure and raised its own must report " \
                 "that one, not have the block's failure re-raised over it")
  end

  def test_b24_a_rescued_block_raise_leaves_nothing_behind
    seen = rescuing_sandbox(raising: false).eval(<<~RUBY).value
      first = Probe::Swallow.call { raise "swallowed" }
      first.to_s + ' then ' + Probe::Echo.call(42).to_s
    RUBY

    assert_equal "rescued then 42", seen,
                 "a block failure the Service rescued is spent, so a later call on the same " \
                 "invocation must answer normally rather than re-raising it"
  end

  private

  # A Service that yields and lets whatever the block raises through.
  def yielding_sandbox
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("Probe::Boom", ->(&blk) { blk.call })
    sandbox
  end

  # A Service that rescues its block's failure — then either raises its
  # own in its place, or answers normally. +Probe::Echo+ is the later call
  # that witnesses whether the rescued failure left anything behind.
  def rescuing_sandbox(raising:)
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("Probe::Swallow", lambda do |&blk|
      blk.call
    rescue StandardError
      raise IOError, "the Service failed on its own" if raising

      :rescued
    end)
    sandbox.bind("Probe::Echo", ->(value, &_blk) { value })
    sandbox
  end
end
