# frozen_string_literal: true

# Layer 3 unit tests for Kobako::Registry::ServiceGroup edge cases and
# Sandbox outcome-attribution edge cases not covered by the existing suite.
#
# Does NOT require the native extension: ServiceGroup is pure Ruby, and
# the outcome-attribution tests use Sandbox.allocate + send to bypass the
# wasmtime pipeline.
#
# Cross-references:
#   - SPEC.md §B-07 — Group name validation
#   - SPEC.md §B-08 — Member name validation (non-symbol/non-string input)
#   - SPEC.md §B-10 — define idempotence (duplicate Group)
#   - SPEC.md §B-11 — duplicate bind raises, existing binding preserved
#   - SPEC.md §E-09 / §Error Scenarios — unknown Panic origin maps to SandboxError

require "minitest/autorun"
require_relative "support/outcome_bytes_helpers"

$LOAD_PATH.unshift File.expand_path("../lib", __dir__)
require "kobako/registry"
require "kobako/sandbox"
require "kobako/wire"
require "kobako/errors"

# ---------------------------------------------------------------------------
# ServiceGroup edge cases
# ---------------------------------------------------------------------------

module Kobako
  class ServiceGroupEdgeCasesTest < Minitest::Test
    Group = Kobako::Registry::ServiceGroup

    # --- bind: non-symbol/non-string member name ---

    # SPEC §B-08 Notes: bind validates the member name against the constant
    # pattern after coercing it to a String via #to_s.  Passing an Integer
    # hits the coercion path ("42".to_s → "42") then fails the pattern check
    # because "42" starts with a digit, not an uppercase letter.
    def test_bind_with_integer_member_name_raises_argument_error
      group = Group.new("Logger")
      err = assert_raises(ArgumentError) { group.bind(42, Object.new) }
      assert_match(/MemberName/, err.message)
    end

    # An Array as member name produces a String via #to_s that cannot match
    # the constant pattern — another non-symbol/non-string path.
    def test_bind_with_array_member_name_raises_argument_error
      group = Group.new("Logger")
      assert_raises(ArgumentError) { group.bind([:bad], Object.new) }
    end

    # --- duplicate bind (B-11) ---

    # Already covered in test_service_registry.rb (via Sandbox#define + bind),
    # but that test requires the native ext.  This version exercises the same
    # guarantee at the ServiceGroup level without any ext dependency.
    def test_duplicate_bind_at_group_level_raises_and_preserves_original
      group = Group.new("Logger")
      group.bind(:Info, :first)

      err = assert_raises(ArgumentError) { group.bind(:Info, :second) }
      assert_match(/already bound/, err.message)
      assert_equal :first, group.fetch("Info"),
                   "original binding must survive duplicate-bind attempt"
    end

    # --- empty group: to_preamble round-trip ---

    # SPEC §B-07 Notes: an empty Group (no Members) is legal and its
    # to_preamble form is [name, []].  Verifies that guest_preamble does not
    # blow up on a Registry that contains only empty Groups.
    def test_empty_group_to_preamble_returns_empty_members_list
      group = Group.new("Empty")
      assert_equal ["Empty", []], group.to_preamble
    end

    def test_registry_with_only_empty_group_produces_valid_preamble
      require "msgpack"
      registry = Kobako::Registry.new
      registry.define(:Empty)

      bytes = registry.guest_preamble
      decoded = MessagePack.unpack(bytes)
      assert_equal [["Empty", []]], decoded
    end

    # --- non-symbol string name accepted by bind ---

    # SPEC §B-08: both Symbol and String forms of a constant-pattern name
    # must be accepted and normalized to the same String key internally.
    def test_bind_with_string_member_name_normalizes_to_string_key
      group = Group.new("Logger")
      group.bind("Info", :v)
      assert_equal :v, group.fetch("Info")
      assert_equal :v, group["Info"]
    end

    # --- ServiceGroup#[] returns nil for missing member ---

    def test_bracket_returns_nil_for_unknown_member
      group = Group.new("Logger")
      group.bind(:Info, :val)
      assert_nil group["Missing"]
    end

    # --- ServiceGroup#fetch raises KeyError for missing member ---

    def test_fetch_raises_key_error_for_unknown_member
      group = Group.new("Logger")
      err = assert_raises(KeyError) { group.fetch("Unknown") }
      assert_match(/Unknown/, err.message)
    end
  end
end

# ---------------------------------------------------------------------------
# Sandbox outcome-attribution edge cases (Layer 3 — no wasmtime pipeline)
# ---------------------------------------------------------------------------

class TestSandboxOutcomeAttributionEdgeCases < Minitest::Test
  include OutcomeBytesHelpers

  # Decode a raw outcome byte-string through the OutcomeDecoder module
  # without building a wasmtime pipeline.
  def decode(bytes)
    Kobako::Sandbox::OutcomeDecoder.decode(bytes)
  end

  # --- Panic with unknown origin (SPEC §E-09 / §Error Scenarios) ---
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
    panic = Kobako::Wire::Envelope::Panic.new(
      origin: "sandbox", klass: "RuntimeError", message: "box-side error"
    )
    bytes = build_outcome_bytes(
      Kobako::Wire::Envelope::OUTCOME_TAG_PANIC,
      Kobako::Wire::Envelope.encode_panic(panic)
    )

    err = assert_raises(Kobako::SandboxError) { decode(bytes) }
    refute_kind_of Kobako::ServiceError, err
    assert_equal "box-side error", err.message
  end

  # --- Panic with missing "class" field raises SandboxError (SPEC §E-08) ---
  #
  # decode_panic calls Envelope.decode_panic, which raises Wire::InvalidType
  # when a required key is absent.  The Sandbox rescue chain wraps that as
  # SandboxError with klass="Kobako::WireError".
  def test_panic_with_missing_class_field_raises_sandbox_error
    # Hand-roll a panic map that omits "class" — cannot use Panic.new because
    # it requires the field; build the raw bytes directly.
    raw_map = Kobako::Wire::Encoder.encode(
      "origin" => "sandbox", "message" => "missing class"
    )
    bytes = build_outcome_bytes(
      Kobako::Wire::Envelope::OUTCOME_TAG_PANIC, raw_map
    )

    err = assert_raises(Kobako::SandboxError) { decode(bytes) }
    refute_kind_of Kobako::TrapError, err
    assert_equal "Kobako::WireError", err.klass
  end

  # --- Result envelope with empty bytes body raises SandboxError (SPEC §E-09) ---
  #
  # An empty result body is not a valid msgpack value, so decode_result raises
  # Wire::Truncated (a Wire::Error subclass).  The Sandbox rescue chain wraps
  # that as SandboxError (E-09: result envelope decode failed).
  def test_result_envelope_with_empty_body_raises_sandbox_error
    bytes = build_outcome_bytes(
      Kobako::Wire::Envelope::OUTCOME_TAG_RESULT, "".b
    )

    err = assert_raises(Kobako::SandboxError) { decode(bytes) }
    refute_kind_of Kobako::TrapError, err
    assert_equal "Kobako::WireError", err.klass
    assert_match(/result envelope decode failed/, err.message)
  end
end
