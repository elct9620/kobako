# frozen_string_literal: true

# Layer 3 unit tests for the Kobako::Outcome attribution edge cases that
# don't need a live wasmtime pipeline. Attribution is a stateless module
# method, so each test hands it the arm the native side names — no
# Sandbox.
#
# Cross-references:
#   - docs/behavior/errors.md § Error Scenarios — the Step 2 arm table, where
#     a Panic origin other than "service" maps to SandboxError
#   - docs/behavior/errors.md E-09 — an ok payload the codec cannot read

require "test_helper"

class TestOutcomeAttributionEdgeCases < Minitest::Test
  def reify_panic(origin:, klass:, message:, payload: "".b)
    Kobako::Outcome.reify(:panic, payload, [origin, klass, message, []])
  end

  # --- Panic with unknown origin (errors.md § Error Scenarios, Step 2) ---
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

  # --- ok arm with an empty payload raises Transport::Error (E-09) ---
  #
  # An empty payload is not a valid msgpack value, so the codec raises
  # and the host wraps it as a Transport::Error whose user-facing message
  # stays in caller vocabulary; the inner codec diagnostic reaches an
  # operator through Ruby's own +#detailed_message+ channel.
  def test_ok_arm_with_an_empty_payload_raises_sandbox_error
    err = assert_raises(Kobako::Transport::Error) { Kobako::Outcome.reify(:ok, "".b, nil) }

    refute_kind_of Kobako::TrapError, err
    assert_kind_of Kobako::SandboxError, err
    assert_equal "Kobako::Transport::Error", err.klass
    assert_match(/Sandbox produced an invalid result value/, err.message)
    refute_match(/envelope|decode failed/, err.message,
                 "internal codec vocabulary must not leak into the user-facing message")
    refute_equal err.message, err.detailed_message(highlight: false),
                 "an unreadable Result through reify must carry its codec diagnostic on #detailed_message"
  end
end
