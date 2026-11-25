# frozen_string_literal: true

require "test_helper"

# E2E — the most values one guest payload may make the host build, the
# codec's own 262144 whatever the memory cap.
class TestSandboxValueBudget < Minitest::Test
  include GuestGuard

  REAL_WASM = TestPaths.data("kobako.wasm")
  # 1 + 512 arrays + 512 * 511 Integers = 262145 values, one past the
  # budget; popping one Integer leaves exactly 262144.
  PAST_BUDGET = "Array.new(512) { Array.new(511, 0) }"
  AT_BUDGET = "a = #{PAST_BUDGET}; a[0].pop; a".freeze
  NAN_KEYS = "h = {}; %d.times { h[0.0 / 0.0] = 0 }; h"

  def setup
    require_guest_binary(REAL_WASM, build: "bundle exec rake wasm:build")
  end

  # @behavior S-204
  def test_an_answer_at_the_value_budget_reaches_the_host
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM, memory_limit: 64 << 20)

    assert_equal 512, sandbox.eval(AT_BUDGET).value.size,
                 "an answer holding 262144 values through Sandbox#eval must reach the Host App"
  end

  # @behavior S-204
  def test_an_answer_past_the_value_budget_fails_whatever_the_memory_cap
    refusals = [64 << 20, nil].map do |memory_limit|
      sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM, memory_limit: memory_limit)
      assert_raises(Kobako::SandboxError) { sandbox.eval(PAST_BUDGET) }.detailed_message
    end

    refusals.each do |message|
      assert_match(/payload holds more than 262144 values/, message,
                   "an answer holding 262145 values through Sandbox#eval under memory_limit: 64 MiB or " \
                   "nil must fail as a Sandbox failure whose diagnostic names the budget")
    end
  end

  # 500 NaN keys cost about 126 thousand values, 1000 about half a million.
  # @behavior S-219
  def test_a_hash_of_nan_keys_pays_for_comparing_them
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM, memory_limit: 64 << 20)

    assert_equal 500, sandbox.eval(format(NAN_KEYS, 500)).value.size,
                 "a Hash of 500 NaN keys through Sandbox#eval must reach the Host App"
    error = assert_raises(Kobako::SandboxError) { sandbox.eval(format(NAN_KEYS, 1000)) }
    assert_match(/payload holds more than 262144 values/, error.detailed_message,
                 "a Hash of 1000 NaN keys through Sandbox#eval must fail as a Sandbox failure whose " \
                 "diagnostic names the budget")
  end
end
