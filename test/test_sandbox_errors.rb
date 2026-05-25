# frozen_string_literal: true

require "test_helper"

# Outcome-attribution unit coverage for the branches that don't need a
# live Sandbox: zero-length / unknown-tag wire violations, malformed
# envelope payloads, and Panic envelope class-to-Ruby-class mapping
# (including the +BytecodeError+ subclass selection). The
# decode logic lives on +Kobako::Outcome+ as a stateless module of pure
# functions, so we call it directly without instantiating Sandbox.
class TestSandboxOutcomeDecoding < Minitest::Test
  include OutcomeBytesHelpers

  def decode(bytes)
    Kobako::Outcome.decode(bytes)
  end

  # SPEC.md ABI Signatures: "len == 0 is a wire violation; host walks trap path."
  # Empty outcome bytes have no tag → the host emits TrapError. The user-
  # facing message stays in caller vocabulary — "len=0" is a wire-codec
  # detail Host Apps can't act on, so it never appears in +message+.
  def test_zero_length_outcome_bytes_raises_trap_error
    err = assert_raises(Kobako::TrapError) { decode("".b) }

    assert_match(/Sandbox exited without producing a result/, err.message,
                 "len=0 outcome → TrapError attributed to the Sandbox, not to the wire tag byte")
  end

  # SPEC.md Error Scenarios: unknown outcome tag → TrapError (wire
  # violation fallback). Hex tag value belongs in operator-side
  # diagnostics, not the user-facing message — the contract here is "an
  # unrecognised result means the guest runtime is corrupted; discard
  # the Sandbox", phrased in caller vocabulary.
  def test_unknown_outcome_tag_raises_trap_error
    bytes = String.new(encoding: Encoding::ASCII_8BIT)
    bytes << 0xff.chr(Encoding::ASCII_8BIT)

    err = assert_raises(Kobako::TrapError) { decode(bytes) }
    assert_match(/Sandbox produced an unrecognised result/, err.message)
    assert_match(/runtime is corrupted/, err.message)
    refute_match(/0xff/i, err.message,
                 "raw tag byte must not leak into the user-facing message")
  end

  def test_malformed_result_envelope_raises_sandbox_error
    bytes = String.new(encoding: Encoding::ASCII_8BIT)
    bytes << Kobako::Outcome::TYPE_VALUE.chr(Encoding::ASCII_8BIT)
    # Garbage payload that is not valid msgpack.
    bytes << "\xc1\xc1\xc1".b

    err = assert_raises(Kobako::Transport::WireError) { decode(bytes) }
    refute_kind_of Kobako::TrapError, err
    assert_kind_of Kobako::SandboxError, err,
                   "WireError must remain rescuable as SandboxError for callers " \
                   "that don't distinguish wire-violation from script failure"
    assert_equal "Kobako::Transport::WireError", err.klass
    assert_equal "sandbox", err.origin
  end

  def test_malformed_panic_envelope_raises_sandbox_error
    bytes = String.new(encoding: Encoding::ASCII_8BIT)
    bytes << Kobako::Outcome::TYPE_PANIC.chr(Encoding::ASCII_8BIT)
    # Garbage payload that is not a valid panic-shaped msgpack map.
    bytes << "\xc1\xc1\xc1".b

    err = assert_raises(Kobako::Transport::WireError) { decode(bytes) }
    refute_kind_of Kobako::TrapError, err
    assert_kind_of Kobako::SandboxError, err,
                   "WireError must remain rescuable as SandboxError"
    assert_equal "Kobako::Transport::WireError", err.klass
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

  def test_result_envelope_returns_value
    body = Kobako::Codec::Encoder.encode(42)
    bytes = String.new(encoding: Encoding::ASCII_8BIT)
    bytes << Kobako::Outcome::TYPE_VALUE.chr(Encoding::ASCII_8BIT)
    bytes << body

    assert_equal 42, decode(bytes)
  end

  # docs/behavior.md Error Classes + E-37 / E-38: a sandbox-origin Panic
  # whose +class+ field names +Kobako::BytecodeError+ resolves to the
  # BytecodeError subclass, letting Host Apps rescue bytecode-specific
  # failures separately from generic SandboxError. Pins
  # +Outcome.panic_target_class+ — the branch that selects the bytecode
  # subclass over the SandboxError parent on a non-service origin.
  def test_panic_envelope_with_bytecode_klass_dispatches_bytecode_subclass
    bytes = panic_outcome_bytes(
      origin: "sandbox", klass: "Kobako::BytecodeError",
      message: "RITE version mismatch", backtrace: ["(snippet:Helper):1"]
    )

    err = assert_raises(Kobako::BytecodeError) { decode(bytes) }
    assert_kind_of Kobako::SandboxError, err,
                   "BytecodeError must remain a SandboxError subclass"
    assert_equal "sandbox", err.origin
    assert_equal "Kobako::BytecodeError", err.klass
  end
end
