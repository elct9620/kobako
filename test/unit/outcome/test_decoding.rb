# frozen_string_literal: true

require "test_helper"

# Attribution coverage for the branches that don't need a live Sandbox:
# the two arms that carry no record, an unreadable Result payload, and
# the Panic class-to-Ruby-class mapping (including the +BytecodeError+
# subclass selection). Attribution lives on +Kobako::Outcome+ as a
# stateless module of pure functions, so the arms the native side names
# are handed to it directly.
class TestOutcomeDecoding < Minitest::Test
  # One panic arm's attribution fields, in the order the native side
  # hands them over.
  def panic(origin:, klass:, message:, backtrace: [])
    [origin, klass, message, backtrace]
  end

  # docs/behavior/errors.md E-02: a guest that wrote nothing is a wire
  # violation → the host emits TrapError. The user-facing message stays
  # in caller vocabulary — "len=0" is a wire-codec detail Host Apps
  # cannot act on, so it never appears in +message+.
  def test_an_absent_outcome_raises_trap_error
    err = assert_raises(Kobako::TrapError) { Kobako::Outcome.reify(:absent, "".b, nil) }

    assert_match(/Sandbox exited without producing a result/, err.message,
                 "a guest that produced no outcome must attribute to the Sandbox, not to the wire")
  end

  # docs/behavior/errors.md E-03: bytes the envelope cannot frame →
  # TrapError. The contract is "an unrecognised result means the guest
  # runtime is corrupted; discard the Sandbox", phrased in caller
  # vocabulary — the raw bytes are not actionable.
  def test_a_malformed_outcome_raises_trap_error
    err = assert_raises(Kobako::TrapError) { Kobako::Outcome.reify(:malformed, "".b, nil) }

    assert_match(/Sandbox produced an unrecognised result/, err.message)
    assert_match(/runtime is corrupted/, err.message,
                 "an unframeable outcome must tell the caller to discard the Sandbox")
  end

  def test_a_result_payload_the_adapter_cannot_read_raises_sandbox_error
    err = assert_raises(Kobako::Transport::Error) do
      Kobako::Outcome.reify(:result, "\xc1\xc1\xc1".b, nil)
    end

    refute_kind_of Kobako::TrapError, err
    assert_kind_of Kobako::SandboxError, err,
                   "Transport::Error must remain rescuable as SandboxError for callers " \
                   "that don't distinguish wire-violation from script failure"
    assert_equal "Kobako::Transport::Error", err.klass
    assert_equal "sandbox", err.origin
  end

  def test_a_result_payload_returns_the_carried_value
    assert_equal 42, Kobako::Outcome.reify(:result, Kobako::Codec::Encoder.encode(42), nil),
                 "a Result arm through #reify must return the value the guest produced"
  end

  def test_a_service_origin_panic_raises_service_error
    fields = panic(origin: "service", klass: "Kobako::ServiceError",
                   message: "boom", backtrace: ["x:1"])

    err = assert_raises(Kobako::ServiceError) { Kobako::Outcome.reify(:panic, "".b, fields) }

    assert_equal "boom", err.message
    assert_equal "service", err.origin,
                 "a service-origin Panic must attribute the failure to the Service"
  end

  # docs/behavior/errors.md Error Classes + E-37 / E-38: a sandbox-origin
  # Panic naming +Kobako::BytecodeError+ resolves to the BytecodeError
  # subclass, letting Host Apps rescue bytecode-specific failures
  # separately from generic SandboxError.
  def test_a_bytecode_class_panic_raises_the_bytecode_subclass
    fields = panic(origin: "sandbox", klass: "Kobako::BytecodeError",
                   message: "RITE version mismatch", backtrace: ["(snippet:Helper):1"])

    err = assert_raises(Kobako::BytecodeError) { Kobako::Outcome.reify(:panic, "".b, fields) }

    assert_kind_of Kobako::SandboxError, err,
                   "BytecodeError must remain a SandboxError subclass"
    assert_equal "sandbox", err.origin
    assert_equal "Kobako::BytecodeError", err.klass
  end

  # docs/behavior/errors.md E-08: a Panic whose diagnostics violate the
  # wire is not a record worth attributing from, so it takes the
  # invalid-record channel rather than the class its origin names.
  def test_panic_details_the_adapter_cannot_read_raise_an_invalid_record
    fields = panic(origin: "sandbox", klass: "RuntimeError", message: "boom")

    err = assert_raises(Kobako::Transport::Error) { Kobako::Outcome.reify(:panic, "\xc1".b, fields) }

    assert_match(/Sandbox produced an invalid panic record/, err.message,
                 "a Panic whose diagnostics violate the wire must take the invalid-record channel")
  end

  # E-50: the Fault envelope's sole legal wire position is a Reply's
  # fault arm. A Panic smuggling one in its details would hand host code
  # a Fault whose own details can nest Handles nothing outside the wire
  # layer can resolve.
  def test_panic_details_carrying_a_fault_raise_an_invalid_record
    fields = panic(origin: "sandbox", klass: "RuntimeError", message: "boom")
    smuggled = Kobako::Codec::Encoder.encode(Kobako::Fault.new(type: "runtime", message: "smuggled"))

    err = assert_raises(Kobako::Transport::Error) { Kobako::Outcome.reify(:panic, smuggled, fields) }

    assert_match(/Sandbox produced an invalid panic record/, err.message,
                 "E-50: an ext 0x02 in a Panic's details must surface as an invalid panic record")
  end

  def test_panic_details_reach_the_error_when_the_adapter_can_read_them
    fields = panic(origin: "sandbox", klass: "Kobako::SandboxError", message: "undefined entrypoint: Missing")
    details = Kobako::Codec::Encoder.encode({ "available" => %i[Worker] })

    err = assert_raises(Kobako::SandboxError) { Kobako::Outcome.reify(:panic, details, fields) }

    assert_equal({ "available" => %i[Worker] }, err.details,
                 "a Panic's structured diagnostics must reach the raised error intact")
  end
end
