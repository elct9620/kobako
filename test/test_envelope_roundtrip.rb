# frozen_string_literal: true

# Cross-side envelope round-trip E2E (SPEC.md F-05 / F-09).
#
# Drives the Rust `envelope_oracle` subprocess from the host: each test
# Ruby-encodes one transport envelope variant (Request, Response), prefixes a
# single-byte kind tag, and asks the oracle to decode + re-encode it.
# The Ruby side then asserts byte-identical round-trip — proving the
# host and guest envelope modules agree on the SPEC framing (field
# order, tag bytes, optional-field handling), not just the underlying
# msgpack codec already covered by test_codec_roundtrip_fuzz.rb.
#
# Outcome-path envelopes (Result / Panic / Outcome) are not covered
# here: the host never emits them in production — only the Rust guest
# does — so there is no lib-level Ruby encoder for the oracle to
# round-trip against. The host-side decode path is exercised through
# +Kobako::Outcome.decode+ unit tests against hand-rolled bytes.
#
# This test does NOT need fuzz scale: a handful of representative
# envelopes per variant is enough; the codec fuzz in
# test_codec_roundtrip_fuzz.rb already covers byte-level wire shapes
# underneath.

require "test_helper"

class TestEnvelopeRoundtrip < Minitest::Test
  Envelope = Kobako::Transport
  Handle   = Kobako::Handle
  Exc      = Kobako::Fault

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
  # ('Q' Request, 'P' Response).
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
    bytes = encode_request(Handle.restore(7), "save", [], {})
    assert_equal bytes, oracle_roundtrip("Q", bytes)
  end

  def test_request_with_handles_in_args_round_trips
    bytes = encode_request("G::M", "link", [Handle.restore(1), Handle.restore(2)], { k: Handle.restore(3) })
    assert_equal bytes, oracle_roundtrip("Q", bytes)
  end

  def test_request_empty_round_trips
    bytes = encode_request("G::M", "ping", [], {})
    assert_equal bytes, oracle_roundtrip("Q", bytes)
  end

  def encode_request(target, method, args, kwargs)
    Envelope::Request.new(target: target, method_name: method, args: args, kwargs: kwargs).encode
  end

  # ---------- Response ----------

  def test_response_ok_primitive_round_trips
    bytes = Envelope::Response.ok(42).encode
    assert_equal bytes, oracle_roundtrip("P", bytes)
  end

  def test_response_ok_handle_round_trips
    bytes = Envelope::Response.ok(Handle.restore(99)).encode
    assert_equal bytes, oracle_roundtrip("P", bytes)
  end

  def test_response_error_round_trips
    exc = Exc.new(type: "runtime", message: "boom", details: nil)
    bytes = Envelope::Response.error(exc).encode
    assert_equal bytes, oracle_roundtrip("P", bytes)
  end
end
