# frozen_string_literal: true

require "test_helper"

# A Host App's mistake in a call raises the class CRuby's core methods raise
# for it, before any guest runs.
class TestE2EHostAppErrors < Minitest::Test
  include E2eGuestHelper

  # @behavior S-212
  def test_a_registration_after_the_seal_raises_frozen_error
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.eval("1")

    [-> { sandbox.bind("Late::Service", Object.new) },
     -> { sandbox.preload(code: "1", name: "Late") },
     -> { sandbox.flag(:late, "1") }].each do |register|
      assert_raises(FrozenError, "a registration on a Sandbox that has run must raise FrozenError") do
        register.call
      end
    end
  end

  # @behavior S-213
  def test_an_argument_of_another_class_raises_type_error
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)

    [-> { sandbox.preload(code: 1, name: "Snip") },
     -> { sandbox.preload(code: "1", name: 1) },
     -> { sandbox.flag(1, "1") },
     -> { sandbox.flag(:name, 1) },
     -> { sandbox.run(:Entry, **{ "key" => 1 }) }].each do |call|
      assert_raises(TypeError, "a call given an argument of a class it does not take must raise TypeError") do
        call.call
      end
    end
  end

  # @behavior S-214
  def test_a_cap_past_what_the_host_counts_raises_range_error
    assert_raises(RangeError, "Sandbox.new with memory_limit: 2**64 must raise RangeError") do
      Kobako::Sandbox.new(wasm_path: REAL_WASM, memory_limit: 2**64)
    end
    assert_raises(TypeError, "Sandbox.new with timeout: of another class must raise TypeError") do
      Kobako::Sandbox.new(wasm_path: REAL_WASM, timeout: "soon")
    end
  end
end
