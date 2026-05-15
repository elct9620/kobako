# frozen_string_literal: true

# Layer 3 unit tests for Kobako::Outcome edge cases that
# don't need a live wasmtime pipeline. Uses Sandbox.allocate + send to
# bypass instance construction.
#
# Cross-references:
#   - SPEC.md E-09 / Error Scenarios — unknown Panic origin maps to SandboxError
#   - SPEC.md E-08 — missing required key in Panic envelope
#   - SPEC.md Wire Codec — Result envelope decode failures map to SandboxError

require "minitest/autorun"
require_relative "support/outcome_bytes_helpers"

$LOAD_PATH.unshift File.expand_path("../lib", __dir__)
require "kobako/sandbox"
require "kobako/rpc/envelope"
require "kobako/errors"

class TestSandboxOutcomeAttributionEdgeCases < Minitest::Test
  include OutcomeBytesHelpers

  # Decode a raw outcome byte-string through the Kobako::Outcome module
  # without building a wasmtime pipeline.
  def decode(bytes)
    Kobako::Outcome.decode(bytes)
  end

  # --- Panic with unknown origin (SPEC E-09 / Error Scenarios) ---
  #
  # SPEC: origin values other than "service" and "sandbox" are treated as
  # sandbox-side failures (the panic_target_class method returns SandboxError
  # for any origin that is not exactly "service").  This is the third branch
  # of the origin decision tree.
  def test_panic_with_unknown_origin_raises_sandbox_error
    bytes = panic_outcome_bytes(origin: "unknown", klass: "Kobako::SomeError", message: "strange")

    err = assert_raises(Kobako::SandboxError) { decode(bytes) }
    refute_kind_of Kobako::ServiceError, err,
                   "unknown origin must not produce ServiceError"
    assert_equal "strange", err.message
    # The unknown origin value is propagated verbatim (not overwritten).
    assert_equal "unknown", err.origin
  end

  # --- Panic with origin "sandbox" explicitly raises SandboxError (not ServiceError) ---
  #
  # Belt-and-suspenders: pin the canonical "sandbox" origin path at unit
  # level, independent of the fixture-driven test in test_sandbox_errors.rb.
  def test_panic_with_sandbox_origin_raises_sandbox_error_not_service_error
    panic = Kobako::Outcome::Panic.new(
      origin: "sandbox", klass: "RuntimeError", message: "box-side error"
    )
    bytes = build_outcome_bytes(Kobako::Outcome::TYPE_PANIC, encode_panic_body(panic))

    err = assert_raises(Kobako::SandboxError) { decode(bytes) }
    refute_kind_of Kobako::ServiceError, err
    assert_equal "box-side error", err.message
  end

  # --- Panic with missing "class" field raises SandboxError (SPEC E-08) ---
  #
  # decode_panic calls Envelope.decode_panic, which raises Kobako::Codec::InvalidType
  # when a required key is absent.  The Sandbox rescue chain wraps that as
  # SandboxError with klass="Kobako::WireError".
  def test_panic_with_missing_class_field_raises_sandbox_error
    # Hand-roll a panic map that omits "class" — cannot use Panic.new because
    # it requires the field; build the raw bytes directly.
    raw_map = Kobako::Codec::Encoder.encode(
      "origin" => "sandbox", "message" => "missing class"
    )
    bytes = build_outcome_bytes(Kobako::Outcome::TYPE_PANIC, raw_map)

    err = assert_raises(Kobako::SandboxError) { decode(bytes) }
    refute_kind_of Kobako::TrapError, err
    assert_equal "Kobako::WireError", err.klass
  end

  # --- Result envelope with empty bytes body raises SandboxError (SPEC E-09) ---
  #
  # An empty result body is not a valid msgpack value, so decode_result raises
  # Kobako::Codec::Truncated (a Kobako::Codec::Error subclass).  The Sandbox rescue chain wraps
  # that as SandboxError (E-09: result envelope decode failed).
  def test_result_envelope_with_empty_body_raises_sandbox_error
    bytes = build_outcome_bytes(Kobako::Outcome::TYPE_VALUE, "".b)

    err = assert_raises(Kobako::SandboxError) { decode(bytes) }
    refute_kind_of Kobako::TrapError, err
    assert_equal "Kobako::WireError", err.klass
    assert_match(/result envelope decode failed/, err.message)
  end
end
