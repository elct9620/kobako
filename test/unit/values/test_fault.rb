# frozen_string_literal: true

require "test_helper"

# Kobako::Fault is the value object for a Fault (ext 0x02)
# (docs/wire/payload-msgpack.md § Ext Types → ext 0x02): a {type, message}
# pair whose type is drawn from a closed taxonomy. This file pins the
# constructor's own validation contract; the codec round-trip of the
# encoded form lives in test/unit/codec/test_ext_types.rb.
class TestFault < Minitest::Test
  def test_instances_are_frozen
    fault = Kobako::Fault.new(type: "runtime", message: "boom")

    assert_predicate fault, :frozen?,
                     "a Fault through Kobako::Fault.new must be an immutable wire value object"
  end

  def test_rejects_non_string_type
    err = assert_raises(ArgumentError) { Kobako::Fault.new(type: :runtime, message: "m") }

    assert_match(/type must be String/, err.message,
                 "a non-String type through Kobako::Fault.new must raise ArgumentError")
  end

  def test_rejects_non_string_message
    err = assert_raises(ArgumentError) { Kobako::Fault.new(type: "runtime", message: 42) }

    assert_match(/message must be String/, err.message,
                 "a non-String message through Kobako::Fault.new must raise ArgumentError")
  end

  def test_rejects_type_outside_the_closed_taxonomy
    err = assert_raises(ArgumentError) { Kobako::Fault.new(type: "fatal", message: "m") }

    assert_match(/not one of/, err.message,
                 "a type outside VALID_TYPES through Kobako::Fault.new must raise ArgumentError")
  end
end
