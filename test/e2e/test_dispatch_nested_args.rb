# frozen_string_literal: true

require "test_helper"

# E2E — a nested Array of Hashes crossing a dispatch in both directions
# through real mruby. A conversion correct in one direction only would
# pass either test alone, so each direction is witnessed on its own.
class TestE2EDispatchNestedArgs < Minitest::Test
  include E2eGuestHelper

  NESTED_AOH = [{ x: 1 }, { y: 2 }].freeze

  # @behavior T-152
  def test_rpc_nested_array_of_hash_arrives_natively
    seen, = echo_nested_array_of_hash

    assert_equal NESTED_AOH, seen,
                 "a nested Array-of-Hash passed to a Service through #eval must arrive natively"
  end

  # @behavior T-267
  def test_rpc_nested_array_of_hash_round_trip
    _, result = echo_nested_array_of_hash

    assert_equal NESTED_AOH, result,
                 "a nested Array-of-Hash a Service answers through #eval must round-trip losslessly"
  end

  private

  # Pass an Array of Hashes to an echoing Service, then answer what the
  # Service received and what the guest got back.
  def echo_nested_array_of_hash
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    seen = []
    sandbox.bind("Echo::Identity", ->(arg) { arg.tap { seen << arg } })
    result = sandbox.eval("Echo::Identity.call([{x: 1}, {y: 2}])").value
    [seen.first, result]
  end
end
