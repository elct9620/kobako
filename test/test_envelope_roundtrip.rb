# frozen_string_literal: true

# Cross-side envelope round-trip E2E (SPEC item #8).
#
# Builds the `envelope_oracle` Rust binary, spawns it as a long-lived
# subprocess, and feeds it Ruby-encoded envelopes (Request, Response,
# Result, Panic, Outcome). The oracle decodes each envelope with the
# guest envelope module, re-encodes it, and writes the bytes back. The
# Ruby side asserts byte-identical round-trip — proving the host and
# guest envelope modules agree on the SPEC framing (field order, tag
# bytes, optional-field handling), not just the underlying msgpack
# codec already covered by test_codec_roundtrip_fuzz.rb.
#
# This test does NOT need fuzz scale: a handful of representative
# envelopes per variant is enough; the codec fuzz from item #7 already
# covers byte-level wire shapes underneath.

require "minitest/autorun"
require "open3"

$LOAD_PATH.unshift File.expand_path("../lib", __dir__)
require "kobako/wire"

class TestEnvelopeRoundtrip < Minitest::Test
  Envelope = Kobako::Wire::Envelope
  Handle   = Kobako::Wire::Handle
  Exc      = Kobako::Wire::Exception

  PROJECT_ROOT = File.expand_path("..", __dir__)
  CRATE_DIR    = File.join(PROJECT_ROOT, "wasm", "kobako-wasm")
  ORACLE_BIN   = File.join(CRATE_DIR, "target", "release", "envelope_oracle")
  ERROR_FLAG   = 0x8000_0000

  @@build_status = nil
  @@build_error  = nil

  def self.ensure_oracle_built
    @@build_status ||= cargo_build_oracle
  end

  def self.cargo_build_oracle
    return :no_cargo if `which cargo 2>/dev/null`.strip.empty?

    out, status = Open3.capture2e(
      { "CARGO_TARGET_DIR" => File.join(CRATE_DIR, "target") },
      "cargo", "build", "--release", "--bin", "envelope_oracle",
      chdir: CRATE_DIR
    )
    return :ok if status.success?

    @@build_error = out
    :build_failed
  end

  def setup
    case self.class.ensure_oracle_built
    when :no_cargo
      skip "cargo not on PATH; envelope oracle E2E requires Rust toolchain"
    when :build_failed
      flunk "cargo build --release envelope_oracle failed:\n#{@@build_error}"
    end
    @stdin, @stdout, @wait_thr = Open3.popen2(ORACLE_BIN)
    @stdin.binmode
    @stdout.binmode
  end

  def teardown
    @stdin&.close
    @stdout&.close
    @wait_thr&.value
  end

  # Send one envelope frame to the oracle and read its response.
  # +kind+ is a single-byte tag picked by the oracle protocol
  # ('Q' Request, 'P' Response, 'R' Result, 'X' Panic, 'O' Outcome).
  def oracle_roundtrip(kind, payload)
    send_frame(kind, payload)
    read_response_frame
  end

  def send_frame(kind, payload)
    frame = +"".b << kind << payload.b
    @stdin.write([frame.bytesize].pack("N"))
    @stdin.write(frame)
    @stdin.flush
  end

  def read_response_frame
    hdr = @stdout.read(4) or flunk "oracle stdout closed; no header"
    hdr_word = hdr.unpack1("N")
    len = hdr_word & 0x7fff_ffff
    body = len.zero? ? "".b : @stdout.read(len)
    flunk "oracle stdout truncated (expected #{len} bytes)" if body.nil? || body.bytesize != len
    flunk "oracle reported error: #{body}" if hdr_word.anybits?(ERROR_FLAG)
    body
  end

  # ---------- Request ----------

  def test_request_with_path_target_round_trips
    bytes = Envelope.encode_request("Store::Users", "find", [42, "alice"], { "active" => true })
    assert_equal bytes, oracle_roundtrip("Q", bytes)
  end

  def test_request_with_handle_target_round_trips
    bytes = Envelope.encode_request(Handle.new(7), "save", [], {})
    assert_equal bytes, oracle_roundtrip("Q", bytes)
  end

  def test_request_with_handles_in_args_round_trips
    bytes = Envelope.encode_request("G::M", "link", [Handle.new(1), Handle.new(2)],
                                    { "k" => Handle.new(3) })
    assert_equal bytes, oracle_roundtrip("Q", bytes)
  end

  def test_request_empty_round_trips
    bytes = Envelope.encode_request("G::M", "ping", [], {})
    assert_equal bytes, oracle_roundtrip("Q", bytes)
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
    bytes = Envelope.encode_outcome(Envelope::Outcome.result(123))
    assert_equal bytes, oracle_roundtrip("O", bytes)
  end

  def test_outcome_panic_round_trips
    bytes = Envelope.encode_outcome(Envelope::Outcome.panic(
                                      Envelope::Panic.new(origin: "sandbox",
                                                          klass: "RuntimeError",
                                                          message: "boom")
                                    ))
    assert_equal bytes, oracle_roundtrip("O", bytes)
  end
end
