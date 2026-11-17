# frozen_string_literal: true

require "test_helper"

# E2E — the value budget a memory cap grants a guest payload, driven by a
# fixture whose answer holds six values without growing its memory. The
# real Guest Binary cannot reach the budget: encoding a value costs it
# more memory than the budget grants.
class TestSandboxValueBudget < Minitest::Test
  include GuestGuard

  FIXTURE_PATH = TestPaths.fixture("minimal_wide_outcome.wat")

  def setup
    require_fixture(FIXTURE_PATH)
  end

  # 96 bytes of memory cap grant 96 / 16 = 6 values, exactly what the
  # answer holds.
  # @behavior S-204
  def test_an_answer_at_the_value_budget_reaches_the_host
    sandbox = Kobako::Sandbox.new(wasm_path: FIXTURE_PATH, memory_limit: 96)

    assert_equal [0, 0, 0, 0, 0], sandbox.eval("").value,
                 "an answer holding as many values as memory_limit / 16 through Sandbox#eval must " \
                 "reach the Host App"
  end

  # 80 bytes grant 5 values, one fewer than the answer holds.
  # @behavior S-204
  def test_an_answer_past_the_value_budget_is_a_sandbox_failure
    sandbox = Kobako::Sandbox.new(wasm_path: FIXTURE_PATH, memory_limit: 80)

    error = assert_raises(Kobako::SandboxError) { sandbox.eval("") }

    assert_match(/payload holds more than 5 values/, error.detailed_message,
                 "an answer holding more values than memory_limit / 16 through Sandbox#eval must " \
                 "fail as a Sandbox failure whose diagnostic names the budget")
  end

  REAL_WASM = TestPaths.data("kobako.wasm")
  NAN_KEYS = "h = {}; %d.times { h[0.0 / 0.0] = 0 }; h"

  # 64 MiB grant 4194304 values: 1000 NaN keys cost about half a million,
  # 4000 cost about eight million.
  # @behavior S-219
  def test_a_hash_of_nan_keys_pays_for_comparing_them
    require_guest_binary(REAL_WASM, build: "bundle exec rake wasm:build")
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM, memory_limit: 64 << 20)

    assert_equal 1000, sandbox.eval(format(NAN_KEYS, 1000)).value.size,
                 "a Hash of 1000 NaN keys through Sandbox#eval under memory_limit: 64 MiB must reach the Host App"
    error = assert_raises(Kobako::SandboxError) { sandbox.eval(format(NAN_KEYS, 4000)) }
    assert_match(/payload holds more than 4194304 values/, error.detailed_message,
                 "a Hash of 4000 NaN keys through Sandbox#eval under memory_limit: 64 MiB must fail as a " \
                 "Sandbox failure whose diagnostic names the budget")
  end
end
