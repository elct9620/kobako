# frozen_string_literal: true

require "test_helper"

# Attribution coverage for the branches that don't need a live Sandbox:
# the two arms that carry no record, an unreadable ok payload, and
# the Panic class-to-Ruby-class mapping (including the +BytecodeError+
# and +UndefinedEntrypointError+ subclass selections). Attribution lives
# on +Kobako::Outcome+ as a stateless module of pure functions, so the
# arms the native side names are handed to it directly.
class TestOutcomeDecoding < Minitest::Test
  # One panic arm's fields, in the order the native side hands them over.
  def panic(origin:, klass:, message:, backtrace: [], available: [])
    [origin, klass, message, backtrace, available]
  end

  # @behavior OC-001
  # The message stays in caller vocabulary — a zero length is a wire
  # detail a Host App cannot act on, so it never appears in +message+.
  def test_an_absent_outcome_raises_trap_error
    err = assert_raises(Kobako::TrapError) { Kobako::Outcome.reify(:absent, "".b, nil) }

    assert_match(/Sandbox exited without producing a result/, err.message,
                 "a guest that produced no outcome must attribute to the Sandbox, not to the wire")
  end

  # @behavior OC-002
  # An unrecognised result means the guest runtime is past reasoning
  # about, so what the message has to convey is "discard the Sandbox"
  # rather than the bytes, which are not actionable.
  def test_a_malformed_outcome_raises_trap_error
    err = assert_raises(Kobako::TrapError) { Kobako::Outcome.reify(:malformed, "".b, nil) }

    assert_match(/Sandbox produced an unrecognised result/, err.message)
    assert_match(/runtime is corrupted/, err.message,
                 "an unframeable outcome must tell the caller to discard the Sandbox")
  end

  # @behavior OC-003
  # The guest ran and answered, so the Sandbox is intact — attributing
  # this as a trap would tell the Host App to discard a usable one.
  def test_an_ok_payload_the_codec_cannot_read_raises_sandbox_error
    err = assert_raises(Kobako::Transport::Error) do
      Kobako::Outcome.reify(:ok, "\xc1\xc1\xc1".b, nil)
    end

    refute_kind_of Kobako::TrapError, err
    assert_kind_of Kobako::SandboxError, err,
                   "Transport::Error must remain rescuable as SandboxError for callers " \
                   "that don't distinguish wire-violation from script failure"
    assert_equal "Kobako::Transport::Error", err.klass
    assert_equal "sandbox", err.origin
  end

  # @behavior OC-006
  def test_an_ok_payload_returns_the_carried_value
    assert_equal 42, Kobako::Outcome.reify(:ok, Kobako::Codec::Encoder.encode(42), nil),
                 "an ok arm through #reify must return the value the guest produced"
  end

  # @behavior OC-008
  def test_a_service_origin_panic_raises_service_error
    fields = panic(origin: "service", klass: "Kobako::ServiceError",
                   message: "boom", backtrace: ["x:1"])

    err = assert_raises(Kobako::ServiceError) { Kobako::Outcome.reify(:panic, "".b, fields) }

    assert_equal "boom", err.message
    assert_equal "service", err.origin,
                 "a service-origin Panic must attribute the failure to the Service"
  end

  # @behavior OC-013
  # The subclass is what lets a Host App rescue a bytecode failure
  # separately while a plain SandboxError rescue still covers it.
  def test_a_bytecode_class_panic_raises_the_bytecode_subclass
    fields = panic(origin: "sandbox", klass: "Kobako::BytecodeError",
                   message: "RITE version mismatch", backtrace: ["(snippet:Helper):1"])

    err = assert_raises(Kobako::BytecodeError) { Kobako::Outcome.reify(:panic, "".b, fields) }

    assert_kind_of Kobako::SandboxError, err,
                   "BytecodeError must remain a SandboxError subclass"
    assert_equal "sandbox", err.origin
    assert_equal "Kobako::BytecodeError", err.klass
  end

  # @behavior OC-015
  # Every failure field is typed at the core envelope, so whatever bytes
  # sit in the value slot the failure still names itself — asking the
  # codec would let unreadable bytes rewrite the attribution.
  def test_a_panic_attributes_without_reading_the_value_slot
    fields = panic(origin: "service", klass: "Kobako::ServiceError", message: "connection refused")

    err = assert_raises(Kobako::ServiceError) { Kobako::Outcome.reify(:panic, "\xc1".b, fields) }

    refute_kind_of Kobako::Transport::Error, err,
                   "a panic arm through reify must never settle as a wire violation, whatever the value slot holds"
    assert_equal "connection refused", err.message,
                 "a panic arm through reify must raise with the guest's own message"
  end

  # @behavior OC-014
  # The correction needs both halves — the name asked for, which only the
  # host knows, and the names it could have been, which only the guest
  # does.
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
