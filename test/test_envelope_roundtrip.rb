# frozen_string_literal: true

# Cross-side envelope round-trip E2E (SPEC item #8).
#
# Drives the Rust `envelope_oracle` subprocess from the host: each test
# Ruby-encodes one envelope variant (Request, Response, Result, Panic,
# Outcome), prefixes a single-byte kind tag, and asks the oracle to
# decode + re-encode it. The Ruby side then asserts byte-identical
# round-trip — proving the host and guest envelope modules agree on the
# SPEC framing (field order, tag bytes, optional-field handling), not
# just the underlying msgpack codec already covered by
# test_codec_roundtrip_fuzz.rb.
#
# This test does NOT need fuzz scale: a handful of representative
# envelopes per variant is enough; the codec fuzz from item #7 already
# covers byte-level wire shapes underneath.

require "minitest/autorun"
require_relative "support/cargo_oracle"

$LOAD_PATH.unshift File.expand_path("../lib", __dir__)
require "kobako/wire"

class TestEnvelopeRoundtrip < Minitest::Test
  Envelope = Kobako::Wire::Envelope
  Handle   = Kobako::Wire::Handle
  Exc      = Kobako::Wire::Exception

  CRATE_DIR = File.expand_path("../wasm/kobako-wasm", __dir__)
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
  # ('Q' Request, 'P' Response, 'R' Result, 'X' Panic, 'O' Outcome).
  def oracle_roundtrip(kind, payload)
    @channel.send_frame(+"".b << kind << payload.b)
    body, error = @channel.read_frame
    flunk "oracle reported error: #{body}" if error
    body
  end

  # ---------- Request ----------

  def test_request_with_path_target_round_trips
    bytes = encode_request("Store::Users", "find", [42, "alice"], { active: true })
    assert_equal bytes, oracle_roundtrip("Q", bytes)
  end

  def test_request_with_handle_target_round_trips
    bytes = encode_request(Handle.new(7), "save", [], {})
    assert_equal bytes, oracle_roundtrip("Q", bytes)
  end

  def test_request_with_handles_in_args_round_trips
    bytes = encode_request("G::M", "link", [Handle.new(1), Handle.new(2)], { k: Handle.new(3) })
    assert_equal bytes, oracle_roundtrip("Q", bytes)
  end

  def test_request_empty_round_trips
    bytes = encode_request("G::M", "ping", [], {})
    assert_equal bytes, oracle_roundtrip("Q", bytes)
  end

  def encode_request(target, method, args, kwargs)
    Envelope.encode_request(Envelope::Request.new(target: target, method: method, args: args, kwargs: kwargs))
  end

  # ---------- Response ----------

  def test_response_ok_primitive_round_trips
    bytes = Envelope.encode_response(Envelope::Response.ok(42))
    assert_equal bytes, oracle_roundtrip("P", bytes)
  end

  def test_response_ok_handle_round_trips
    bytes = Envelope.encode_response(Envelope::Response.ok(Handle.new(99)))
    assert_equal bytes, oracle_roundtrip("P", bytes)
  end

  def test_response_err_round_trips
    exc = Exc.new(type: "runtime", message: "boom", details: nil)
    bytes = Envelope.encode_response(Envelope::Response.err(exc))
    assert_equal bytes, oracle_roundtrip("P", bytes)
  end

  # ---------- Result envelope ----------

  def test_result_primitive_round_trips
    bytes = Envelope.encode_result(42)
    assert_equal bytes, oracle_roundtrip("R", bytes)
  end

  def test_result_nil_round_trips
    bytes = Envelope.encode_result(nil)
    assert_equal bytes, oracle_roundtrip("R", bytes)
  end

  def test_result_handle_round_trips
    bytes = Envelope.encode_result(Handle.new(5))
    assert_equal bytes, oracle_roundtrip("R", bytes)
  end

  # ---------- Panic envelope ----------

  def test_panic_minimum_round_trips
    bytes = Envelope.encode_panic(
      Envelope::Panic.new(origin: "sandbox", klass: "RuntimeError", message: "boom")
    )
    assert_equal bytes, oracle_roundtrip("X", bytes)
  end

  def test_panic_with_backtrace_round_trips
    bytes = Envelope.encode_panic(
      Envelope::Panic.new(
        origin: "service",
        klass: "Kobako::ServiceError",
        message: "service failed",
        backtrace: ["a.rb:1", "b.rb:2"]
      )
    )
    assert_equal bytes, oracle_roundtrip("X", bytes)
  end

  # ---------- Outcome envelope ----------

  def test_outcome_result_round_trips
    bytes = Envelope.encode_outcome(Envelope::Outcome.new(123))
    assert_equal bytes, oracle_roundtrip("O", bytes)
  end

  def test_outcome_panic_round_trips
    panic = Envelope::Panic.new(origin: "sandbox", klass: "RuntimeError", message: "boom")
    bytes = Envelope.encode_outcome(Envelope::Outcome.new(panic))
    assert_equal bytes, oracle_roundtrip("O", bytes)
  end
end
