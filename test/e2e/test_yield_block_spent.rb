# frozen_string_literal: true

require "test_helper"

# E2E (Layer 4) — how long a guest block's failure stays available to be
# continued (docs/behavior/yield.md B-24). The guest holds the exception
# only until it learns whether the Service rescued it; once the Service
# runs the block again or the dispatch answers, the failure is spent and
# must not be handed to any later one. What an unrescued failure becomes
# lives in test_yield_block_failure.rb.
class TestE2EYieldBlockSpent < Minitest::Test
  include E2eGuestHelper

  def test_b24_a_rescued_block_raise_leaves_nothing_behind
    seen = swallowing_sandbox.eval(<<~RUBY).value
      first = Probe::Swallow.call { raise "swallowed" }
      first.to_s + ' then ' + Probe::Echo.call(42).to_s
    RUBY

    assert_equal "rescued then 42", seen,
                 "a block failure the Service rescued is spent, so a later call on the same " \
                 "invocation must answer normally rather than re-raising it"
  end

  # A Service that rescues its block's failure and yields again lets guest
  # code run in between — and that code may dispatch. The failure held for
  # the outer block must not answer the inner one, whose block refused a
  # value and so has nothing of its own to continue.
  STOLEN_PROBE = <<~RUBY
    Probe::Twice.call do |n|
      if n == 1
        raise 'the rescued one'
      else
        begin
          Probe::Once.call { Object.new }
        rescue => e
          e.message
        end
      end
    end
  RUBY

  def test_b24_a_held_block_failure_answers_only_its_own_block
    sandbox = twice_sandbox
    sandbox.bind("Probe::Once", ->(&blk) { blk.call(1) })

    seen = sandbox.eval(STOLEN_PROBE).value

    assert_match(/not a supported sandbox value type/, seen,
                 "a nested block that refused a value must hear about that refusal, not " \
                 "inherit the exception an outer block raised and its Service rescued")
  end

  # The same block can fail twice in one dispatch. The second failure here
  # refused a value, so it holds no exception of its own — and the first
  # one, which the Service rescued, is spent rather than available to
  # answer it.
  SPENT_PROBE = <<~RUBY
    Probe::Twice.call do |n|
      n == 1 ? raise('the rescued one') : Object.new
    end
  RUBY

  def test_b24_a_rescued_failure_does_not_answer_the_same_blocks_later_refusal
    err = assert_raises(Kobako::ServiceError) { twice_sandbox.eval(SPENT_PROBE) }

    assert_match(/not a supported sandbox value type/, err.message,
                 "a block refusing a value after an earlier raise its Service rescued must " \
                 "report that refusal, not the exception the Service already handled")
  end

  private

  # A Service that rescues its block's failure and answers normally.
  # +Probe::Echo+ is the later call that witnesses whether the rescued
  # failure left anything behind.
  def swallowing_sandbox
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("Probe::Swallow", lambda do |&blk|
      blk.call
    rescue StandardError
      :rescued
    end)
    sandbox.bind("Probe::Echo", ->(value, &_blk) { value })
    sandbox
  end

  # A Service that yields twice to one block, rescuing the first outcome
  # so the second one runs with the first still recent.
  def twice_sandbox
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("Probe::Twice", lambda do |&blk|
      begin
        blk.call(1)
      rescue StandardError
        nil
      end
      blk.call(2)
    end)
    sandbox
  end
end
