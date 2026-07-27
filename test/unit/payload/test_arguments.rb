# frozen_string_literal: true

# Unit tests for the MessagePack payload codec's invocation arguments.
#
# The core envelope is decoded natively and pinned by the golden vectors
# in crates/kobako-transport; what stays on the Ruby side is this
# layer — the `[args, kwargs]` shape a Call or a Run carries.
#
# Cross-references:
#   - SPEC.md § Wire Codec — Call and Run payloads are a 2-element array
#   - docs/wire/payload-msgpack.md § Payload Positions
#   - docs/behavior/errors.md E-50 — a Fault's only home is a Reply's
#     fault arm, so one inside an argument tree is a wire violation

require "test_helper"

module Kobako
  class PayloadArgumentsTest < Minitest::Test
    Arguments = Kobako::Payload::Arguments

    def test_positional_and_keyword_arguments_round_trip
      arguments = Arguments.new(args: [1, "two", nil], kwargs: { force: true })

      assert_equal arguments, Arguments.decode(arguments.encode),
                   "an invocation payload carrying both argument kinds through Arguments " \
                   "must decode back to an equal value object"
    end

    def test_an_empty_payload_keeps_both_positions
      encoded = Arguments.new.encode

      assert_equal "\x92\x90\x80".b, encoded,
                   "a call with no arguments through Arguments#encode must still emit both " \
                   "the args array and the kwargs map, so field positions stay stable"
      assert_equal Arguments.new, Arguments.decode(encoded),
                   "an empty invocation payload through Arguments.decode must decode as empty " \
                   "rather than as a malformed frame"
    end

    def test_a_kwargs_key_that_is_not_a_symbol_is_refused
      assert_raises(ArgumentError, "a String kwargs key through Arguments.new must be refused — " \
                                   "SPEC pins keyword names to Symbols on the wire") do
        Arguments.new(kwargs: { "force" => true })
      end
    end

    def test_a_non_array_args_is_refused
      assert_raises(ArgumentError, "a non-Array args through Arguments.new must be refused " \
                                   "rather than encoded as some other wire shape") do
        Arguments.new(args: "not an array")
      end
    end

    def test_a_frame_of_the_wrong_arity_is_refused
      bytes = Kobako::Codec::Encoder.encode([[]])

      assert_raises(Kobako::Codec::InvalidTypeError,
                    "a payload that is not a 2-element array through Arguments.decode must be " \
                    "rejected as a wire violation") do
        Arguments.decode(bytes)
      end
    end

    def test_a_fault_smuggled_into_an_argument_is_refused
      fault = Kobako::Fault.new(type: "runtime", message: "boom")
      bytes = Kobako::Codec::Encoder.encode([[fault], {}])

      assert_raises(Kobako::Codec::InvalidTypeError,
                    "a Fault inside an argument through Arguments.decode must be rejected — " \
                    "its only home is a Reply's fault arm") do
        Arguments.decode(bytes)
      end
    end

    def test_a_fault_nested_in_a_kwargs_value_is_refused
      fault = Kobako::Fault.new(type: "runtime", message: "boom")
      bytes = Kobako::Codec::Encoder.encode([[], { cause: [fault] }])

      assert_raises(Kobako::Codec::InvalidTypeError,
                    "a Fault nested inside a kwargs value through Arguments.decode must be " \
                    "rejected as deeply as a bare one") do
        Arguments.decode(bytes)
      end
    end

    def test_a_capability_handle_rides_an_argument_position
      handle = Kobako::Handle.restore(7)
      arguments = Arguments.new(args: [handle], kwargs: { owner: handle })

      assert_equal arguments, Arguments.decode(arguments.encode),
                   "a Capability Handle in either argument position through Arguments must " \
                   "round-trip on the same id"
    end
  end
end
