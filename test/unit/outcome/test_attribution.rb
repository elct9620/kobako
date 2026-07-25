# frozen_string_literal: true

# Layer 3 unit tests for the Kobako::Outcome attribution edge cases that
# don't need a live wasmtime pipeline. Attribution is a stateless module
# method, so each test hands it the arm the native side names — no
# Sandbox.
#
# Cross-references:
#   - SPEC.md E-09 / Error Scenarios — unknown Panic origin maps to SandboxError
#   - SPEC.md Wire Codec — a Result payload the adapter cannot read maps to SandboxError

require "test_helper"

class TestOutcomeAttributionEdgeCases < Minitest::Test
  def reify_panic(origin:, klass:, message:, details: "".b)
    Kobako::Outcome.reify(:panic, details, [origin, klass, message, []])
  end

  # --- Panic with unknown origin (SPEC E-09 / Error Scenarios) ---
  #
  # Origins other than "service" attribute to the sandbox — the third
  # branch of the origin decision tree, and the one an origin the
  # contract does not reserve lands on.
  def test_panic_with_unknown_origin_raises_sandbox_error
    err = assert_raises(Kobako::SandboxError) do
      reify_panic(origin: "unknown", klass: "Kobako::SomeError", message: "strange")
    end

    refute_kind_of Kobako::ServiceError, err,
                   "an origin outside the reserved set must not produce ServiceError"
    assert_equal "strange", err.message
    assert_equal "unknown", err.origin,
                 "the unrecognised origin rides through verbatim rather than being overwritten"
  end

  # Belt-and-suspenders: pin the canonical "sandbox" origin path
  # independently of the fixture-driven cases in test_decoding.rb.
  def test_panic_with_sandbox_origin_raises_sandbox_error_not_service_error
    err = assert_raises(Kobako::SandboxError) do
      reify_panic(origin: "sandbox", klass: "RuntimeError", message: "box-side error")
    end

    refute_kind_of Kobako::ServiceError, err
    assert_equal "box-side error", err.message
  end

  # --- Result arm with an empty payload raises Transport::Error (E-09) ---
  #
  # An empty payload is not a valid msgpack value, so the adapter raises
  # and the host wraps it as a Transport::Error whose user-facing message
  # stays in caller vocabulary; the inner codec diagnostic is preserved
  # under +details+ for operators.
  def test_result_arm_with_an_empty_payload_raises_sandbox_error
    err = assert_raises(Kobako::Transport::Error) { Kobako::Outcome.reify(:result, "".b, nil) }

    refute_kind_of Kobako::TrapError, err
    assert_kind_of Kobako::SandboxError, err
    assert_equal "Kobako::Transport::Error", err.klass
    assert_match(/Sandbox produced an invalid result value/, err.message)
    refute_match(/envelope|decode failed/, err.message,
                 "internal codec vocabulary must not leak into the user-facing message")
    assert_kind_of String, err.details,
                   "operator-side codec diagnostic must be preserved in details"
  end

  # --- Result arm carrying an ext 0x02 Fault raises Transport::Error (E-50) ---
  #
  # The Fault envelope's sole legal wire position is a Reply's fault arm;
  # a Result smuggling one would hand host code a Fault whose details can
  # nest Handles nothing outside the wire layer can resolve. The bare
  # codec stays permissive — the positional rule lives on this decode.
  def test_result_arm_carrying_fault_raises_transport_error
    fault = Kobako::Fault.new(type: "runtime", message: "smuggled")

    err = assert_raises(Kobako::Transport::Error) do
      Kobako::Outcome.reify(:result, Kobako::Codec::Encoder.encode(fault), nil)
    end

    assert_match(/Sandbox produced an invalid result value/, err.message,
                 "E-50: a Result arm carrying ext 0x02 must surface as an invalid result value")
  end
end
