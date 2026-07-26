# frozen_string_literal: true

require "test_helper"

# Attribution coverage for the branches that don't need a live Sandbox:
# the two arms that carry no record, an unreadable Result payload, and
# the Panic class-to-Ruby-class mapping (including the +BytecodeError+
# and +UndefinedEntrypointError+ subclass selections). Attribution lives
# on +Kobako::Outcome+ as a stateless module of pure functions, so the
# arms the native side names are handed to it directly.
class TestOutcomeDecoding < Minitest::Test
  # One panic arm's fields, in the order the native side hands them over.
  def panic(origin:, klass:, message:, backtrace: [], available: [])
    [origin, klass, message, backtrace, available]
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

  # Every Panic field is typed at the core envelope, so a Panic arm never
  # asks the adapter for anything: whatever bytes sit in the value slot,
  # the failure still names itself.
  def test_a_panic_attributes_without_reading_the_value_slot
    fields = panic(origin: "service", klass: "Kobako::ServiceError", message: "connection refused")

    err = assert_raises(Kobako::ServiceError) { Kobako::Outcome.reify(:panic, "\xc1".b, fields) }

    refute_kind_of Kobako::Transport::Error, err,
                   "a panic arm through reify must never settle as a wire violation, whatever the value slot holds"
    assert_equal "connection refused", err.message,
                 "a panic arm through reify must raise with the guest's own message"
  end

  # E-27: an unresolved entrypoint reaches the caller as its own subclass
  # carrying both halves of the correction — the name asked for, which
  # only the host knows, and the names it could have been.
  def test_an_unresolved_entrypoint_raises_the_subclass_carrying_its_correction
    fields = panic(origin: "sandbox", klass: "Kobako::UndefinedEntrypointError",
                   message: "undefined entrypoint: Wrker", available: %w[Worker Helper])

    err = assert_raises(Kobako::UndefinedEntrypointError) do
      Kobako::Outcome.reify(:panic, "".b, fields, entrypoint: :Wrker)
    end

    assert_equal :Wrker, err.name,
                 "an unresolved entrypoint through reify must name what the host asked for"
    assert_equal %i[Worker Helper], err.available,
                 "an unresolved entrypoint through reify must carry the names it could have been, as Symbols"
  end
end
