# frozen_string_literal: true

# Cross-side envelope round-trip E2E (SPEC.md F-05 / F-09).
#
# Drives the Rust `envelope_oracle` subprocess from the host: each test
# Ruby-encodes one envelope variant, prefixes a single-byte kind tag,
# and asks the oracle to decode + re-encode it. The Ruby side then
# asserts byte-identical round-trip — proving the host and guest
# envelope modules agree on the SPEC framing (field order, tag bytes,
# optional-field handling), not just the underlying msgpack codec
# already covered by test/fuzz/test_roundtrip_fuzz.rb.
#
# Transport envelopes (Request / Response) round-trip the production
# +#encode+ output. Outcome-path payloads (Result / Panic / Outcome)
# have no production host-side encoder — the host only decodes them —
# so those frames are assembled by +OutcomeBytesHelpers+, whose byte
# layout is contracted to match the guest encoder; the oracle pins that
# contract against the real Rust implementation.
#
# This test does NOT need fuzz scale: a handful of representative
# envelopes per variant is enough; the codec fuzz in
# test/fuzz/test_roundtrip_fuzz.rb already covers byte-level wire shapes
# underneath.

require "test_helper"

class TestEnvelopeRoundtrip < Minitest::Test
  include OutcomeBytesHelpers

  CRATE_DIR = TestPaths.source("wasm", "kobako-wasm")
  ORACLE    = CargoOracle.new(crate_dir: CRATE_DIR, bin_name: "envelope_oracle")

  def setup
    case (build = ORACLE.ensure_built).status
    when :no_cargo
      skip "cargo not on PATH; envelope oracle E2E requires Rust toolchain"
    when :build_failed
      flunk "cargo build --release envelope_oracle failed:\n#{build.error}"
    end
    @channel = ORACLE.spawn
  end

  def teardown
    @channel&.close
  end

  # Send one envelope frame to the oracle and read its response.
  # +kind+ is a single-byte tag picked by the oracle protocol
  # ('Q' Request, 'P' Response, 'R' Result, 'X' Panic, 'O' Outcome,
  # 'A' invocation Arguments).
  def oracle_roundtrip(kind, payload)
    @channel.send_frame(+"".b << kind << payload.b)
    body, error = @channel.read_frame
    flunk "oracle reported error: #{body}" if error
    body
  end

  # ---------- invocation Arguments payload ----------

  def test_invocation_arguments_round_trip
    bytes = Kobako::Payload::Arguments.new(args: [42, "alice"], kwargs: { active: true }).encode
    assert_equal bytes, oracle_roundtrip("A", bytes),
                 "an args-and-kwargs payload must survive the guest adapter byte-identically"
  end

  def test_invocation_arguments_carrying_a_wrapped_leaf_round_trip
    # A non-wire-representable argument auto-wraps into the Handles table
    # and rides as ext 0x01 in its args position — the payload Handle
    # position docs/wire/payload-msgpack.md § ext 0x01 licenses.
    wrapped = Kobako::Codec::HandleWalk.deep_wrap([Object.new], Kobako::Catalog::Handles.new)
    bytes = Kobako::Payload::Arguments.new(args: wrapped, kwargs: {}).encode
    assert_equal bytes, oracle_roundtrip("A", bytes),
                 "a Handle in an argument position must cross to the guest adapter unchanged"
  end

  # The empty-args/kwargs payload shape is byte-pinned cross-language by
  # the Rust golden vectors, so no oracle case here.

  # ---------- Result envelope (bare codec value) ----------

  def test_result_envelope_round_trips
    bytes = Kobako::Codec::Encoder.encode(["done", 42, { status: :ok }, nil])
    assert_equal bytes, oracle_roundtrip("R", bytes)
  end

  # ---------- Panic body ----------

  # Pins OutcomeBytesHelpers#encode_panic_body byte-for-byte against the
  # guest Panic encoder (field order, backtrace-omitted-when-empty).
  def test_panic_body_round_trips
    panic = Kobako::Outcome::Panic.new(
      origin: "sandbox", klass: "RuntimeError", message: "boom",
      backtrace: ["script.rb:1:in `run'"]
    )
    bytes = encode_panic_body(panic)
    assert_equal bytes, oracle_roundtrip("X", bytes)
  end

  def test_panic_body_with_details_round_trips
    panic = Kobako::Outcome::Panic.new(
      origin: "service", klass: "Kobako::ServiceError", message: "kv missing",
      details: { "key" => "user:1" }
    )
    bytes = encode_panic_body(panic)
    assert_equal bytes, oracle_roundtrip("X", bytes)
  end

  # ---------- Outcome envelope (1-byte tag + branch body) ----------

  def test_outcome_value_envelope_round_trips
    bytes = build_outcome_bytes(Kobako::Outcome::TYPE_VALUE, Kobako::Codec::Encoder.encode("ok"))
    assert_equal bytes, oracle_roundtrip("O", bytes)
  end

  def test_outcome_panic_envelope_round_trips
    bytes = panic_outcome_bytes(
      origin: "sandbox", klass: "ZeroDivisionError", message: "divided by 0",
      backtrace: ["a.rb:3"]
    )
    assert_equal bytes, oracle_roundtrip("O", bytes)
  end
end
