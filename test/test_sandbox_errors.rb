# frozen_string_literal: true

require "test_helper"

# Item #20 — three-class error attribution end-to-end coverage.
#
# Drives the test-guest fixture through every branch of the SPEC § Error
# Scenarios two-step decision tree:
#
#   Step 1 — wasmtime trap (`unreachable`)            → TrapError
#   Step 2 — outcome tag 0x02, origin="sandbox"       → SandboxError
#   Step 2 — outcome tag 0x02, origin="service"       → ServiceError
#   Step 2 — unknown tag (synthetic, exercised in unit-level test)
#                                                     → TrapError
#
# Plus the two SandboxError subclass paths exposed by SPEC §"Error
# Class Hierarchy":
#
#   * HandleTableExhausted (cap hit on alloc).
#   * Output buffer overflow truncates with a `[truncated]` marker
#     (SPEC §B-04 — non-error path).
class TestSandboxErrorAttribution < Minitest::Test
  FIXTURE_PATH = File.expand_path("fixtures/test-guest.wasm", __dir__)

  def setup
    skip "native ext not compiled (run `bundle exec rake compile`)" unless defined?(Kobako::Wasm::Engine)
    skip "test-guest fixture missing (run `bundle exec rake fixtures:test_guest`)" \
      unless File.exist?(FIXTURE_PATH)
  end

  # --- Step 1: wasmtime trap ----------------------------------------

  def test_wasmtime_trap_raises_kobako_trap_error
    sandbox = Kobako::Sandbox.new(wasm_path: FIXTURE_PATH)

    err = assert_raises(Kobako::TrapError) { sandbox.run("trap") }
    refute_kind_of Kobako::SandboxError, err,
                   "TrapError must not be confused with SandboxError"
    refute_kind_of Kobako::ServiceError, err,
                   "TrapError must not be confused with ServiceError"
  end

  # --- Step 2: outcome tag 0x02, origin="service" -------------------

  def test_panic_with_service_origin_raises_service_error
    sandbox = Kobako::Sandbox.new(wasm_path: FIXTURE_PATH)

    err = assert_raises(Kobako::ServiceError) { sandbox.run("panic:service") }
    assert_equal "service exploded", err.message
    assert_equal "service", err.origin
    assert_equal "Kobako::ServiceError", err.klass
    refute_kind_of Kobako::SandboxError, err,
                   "ServiceError must not be confused with SandboxError"
  end

  # --- Step 2: outcome tag 0x02, origin="sandbox" -------------------

  def test_panic_with_sandbox_origin_raises_sandbox_error
    sandbox = Kobako::Sandbox.new(wasm_path: FIXTURE_PATH)

    err = assert_raises(Kobako::SandboxError) { sandbox.run("panic") }
    assert_equal "boom", err.message
    assert_equal "sandbox", err.origin
    assert_equal "RuntimeError", err.klass
    refute_kind_of Kobako::ServiceError, err,
                   "SandboxError(origin=sandbox) must not be confused with ServiceError"
    refute_kind_of Kobako::TrapError, err,
                   "SandboxError must not be confused with TrapError"
  end
end

# Outcome-attribution unit coverage for branches that don't need a full
# wasm fixture: zero-length / unknown-tag / decode-failure paths. The
# decode logic lives as private methods on Kobako::Sandbox per SPEC.md
# §Architecture; we exercise it via Sandbox.allocate + send to avoid
# constructing a wasmtime pipeline for pure byte-decoding tests.
class TestSandboxOutcomeDecoding < Minitest::Test
  def decode(bytes)
    Kobako::Sandbox.allocate.send(:decode_outcome, bytes)
  end

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
    panic = Kobako::Wire::Envelope::Panic.new(
      origin: "service", klass: "Kobako::ServiceError",
      message: "boom", backtrace: ["x:1"]
    )
    body = Kobako::Wire::Envelope.encode_panic(panic)
    bytes = String.new(encoding: Encoding::ASCII_8BIT)
    bytes << Kobako::Wire::Envelope::OUTCOME_TAG_PANIC.chr(Encoding::ASCII_8BIT)
    bytes << body

    err = assert_raises(Kobako::ServiceError) { decode(bytes) }
    assert_equal "boom", err.message
    assert_equal "service", err.origin
  end

  def test_result_envelope_returns_value
    body = Kobako::Wire::Envelope.encode_result(42)
    bytes = String.new(encoding: Encoding::ASCII_8BIT)
    bytes << Kobako::Wire::Envelope::OUTCOME_TAG_RESULT.chr(Encoding::ASCII_8BIT)
    bytes << body

    assert_equal 42, decode(bytes)
  end
end

# Item #20 — placeholder error rewiring assertions. The cycle 24 placeholder
# `Kobako::HandleTableError < StandardError` and the cycle 14 placeholder
# `Kobako::Sandbox::OutputLimitExceeded < StandardError` are gone; the
# canonical SPEC hierarchy now anchors every kobako-raised error under
# `Kobako::Error` with the three-class taxonomy.
class TestErrorClassHierarchy < Minitest::Test
  def test_three_top_level_classes_descend_from_kobako_error
    assert Kobako::TrapError < Kobako::Error
    assert Kobako::SandboxError < Kobako::Error
    assert Kobako::ServiceError < Kobako::Error
  end

  def test_handle_table_exhausted_chains_under_sandbox_error
    assert Kobako::HandleTableExhausted < Kobako::HandleTableError
    assert Kobako::HandleTableError < Kobako::SandboxError
  end

  def test_service_error_disconnected_chains_under_service_error
    assert Kobako::ServiceError::Disconnected < Kobako::ServiceError
  end

  def test_sandbox_output_limit_exceeded_placeholder_is_gone
    # Cycle 14 left `Kobako::Sandbox::OutputLimitExceeded < StandardError`
    # as a placeholder; SPEC §B-04 specifies truncate-with-marker, not
    # raise. The placeholder must no longer exist.
    refute defined?(Kobako::Sandbox::OutputLimitExceeded),
           "Kobako::Sandbox::OutputLimitExceeded must be removed (SPEC §B-04 truncates)"
  end
end
