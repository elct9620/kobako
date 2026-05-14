# frozen_string_literal: true

require "test_helper"

# Outcome-attribution unit coverage for the branches that don't need a
# live Sandbox: zero-length / unknown-tag wire violations, malformed
# envelope payloads, and Panic envelope class-to-Ruby-class mapping
# (including the +ServiceError::Disconnected+ subclass selection). The
# decode logic lives on +Kobako::Sandbox::OutcomeDecoder+ as a stateless
# module of pure functions, so we call it directly without instantiating
# Sandbox.
class TestSandboxOutcomeDecoding < Minitest::Test
  include OutcomeBytesHelpers

  def decode(bytes)
    Kobako::Sandbox::OutcomeDecoder.decode(bytes)
  end

  # SPEC.md ABI Signatures: "len == 0 is a wire violation; host walks trap path."
  # Empty outcome bytes have no tag → the host emits TrapError.
  def test_zero_length_outcome_bytes_raises_trap_error
    err = assert_raises(Kobako::TrapError) { decode("".b) }

    assert_match(/len=0/, err.message,
                 "SPEC.md ABI: len=0 outcome → TrapError with len=0 in message")
  end

  # SPEC.md Error Scenarios: unknown outcome tag → TrapError (wire violation fallback).
  def test_unknown_outcome_tag_raises_trap_error
    bytes = String.new(encoding: Encoding::ASCII_8BIT)
    bytes << 0xff.chr(Encoding::ASCII_8BIT)

    err = assert_raises(Kobako::TrapError) { decode(bytes) }
    assert_match(/unknown outcome tag/, err.message)
  end

  def test_malformed_result_envelope_raises_sandbox_error
    bytes = String.new(encoding: Encoding::ASCII_8BIT)
    bytes << Kobako::Wire::Envelope::OUTCOME_TAG_RESULT.chr(Encoding::ASCII_8BIT)
    # Garbage payload that is not a valid 1-element msgpack array.
    bytes << "\xff\xff\xff".b

    err = assert_raises(Kobako::SandboxError) { decode(bytes) }
    refute_kind_of Kobako::TrapError, err
    assert_equal "Kobako::WireError", err.klass
    assert_equal "sandbox", err.origin
  end

  def test_malformed_panic_envelope_raises_sandbox_error
    bytes = String.new(encoding: Encoding::ASCII_8BIT)
    bytes << Kobako::Wire::Envelope::OUTCOME_TAG_PANIC.chr(Encoding::ASCII_8BIT)
    # Garbage payload that is not a valid panic-shaped msgpack map.
    bytes << "\xff\xff\xff".b

    err = assert_raises(Kobako::SandboxError) { decode(bytes) }
    refute_kind_of Kobako::TrapError, err
    assert_equal "Kobako::WireError", err.klass
  end

  def test_panic_envelope_with_service_origin_dispatches_service_error
    bytes = panic_outcome_bytes(
      origin: "service", klass: "Kobako::ServiceError",
      message: "boom", backtrace: ["x:1"]
    )

    err = assert_raises(Kobako::ServiceError) { decode(bytes) }
    assert_equal "boom", err.message
    assert_equal "service", err.origin
  end

  # SPEC.md E-14 + "Error Classes": a Service-origin Panic whose
  # +class+ field names +Kobako::ServiceError::Disconnected+ resolves to
  # the Disconnected subclass, letting Host Apps rescue the disconnected
  # path specifically. Pins +OutcomeDecoder.panic_target_class+ — the
  # only branch that selects the subclass over the +ServiceError+ parent.
  def test_panic_envelope_with_disconnected_klass_dispatches_disconnected_subclass
    bytes = panic_outcome_bytes(
      origin: "service", klass: "Kobako::ServiceError::Disconnected",
      message: "handle id 7 is disconnected", backtrace: ["x:1"]
    )

    err = assert_raises(Kobako::ServiceError::Disconnected) { decode(bytes) }
    assert_kind_of Kobako::ServiceError, err,
                   "Disconnected must remain a ServiceError subclass"
    assert_equal "service", err.origin
    assert_equal "Kobako::ServiceError::Disconnected", err.klass
  end

  def test_result_envelope_returns_value
    body = Kobako::Wire::Envelope.encode_result(42)
    bytes = String.new(encoding: Encoding::ASCII_8BIT)
    bytes << Kobako::Wire::Envelope::OUTCOME_TAG_RESULT.chr(Encoding::ASCII_8BIT)
    bytes << body

    assert_equal 42, decode(bytes)
  end
end
