# frozen_string_literal: true

# E2E round-trip fuzz harness for the kobako payload codec (SPEC.md F-09).
#
# This is THE proof that the two host-side payload-codec implementations
# (the pure-Ruby `Kobako::Codec` under lib/kobako/codec and the Rust one
# under crates/kobako-codec/src/msgpack) agree on the wire. Fuzz is their
# whole consistency mechanism — there is no shared codec source — and a
# passing run is the release gate per SPEC.md Testing Style Layer 1 (any
# failure blocks release unconditionally).
#
# A third implementation sits on the payload layer that these two never
# meet: the guest's mruby-value walk in
# wasm/kobako-mruby/src/msgpack/convert.rs. Having no peer to differ
# against, it is held to an identity law in test/fuzz/test_guest_value_fuzz.rb
# instead.
#
# Architecture:
#
#   1. Build the Rust oracle binary `roundtrip_oracle` once at test start
#      (cargo build --release).
#   2. Spawn it as a long-lived subprocess via IO.popen and stream
#      length-prefixed frames over stdin/stdout. Per-iteration cargo run
#      would dominate the wall-clock budget.
#   3. For each iteration:
#        a. Generate a random Ruby value with a seeded RNG.
#        b. Encode with `Kobako::Codec::Encoder` -> bytes A.
#        c. Send bytes A to the oracle; receive bytes B (oracle decoded with
#           the Rust codec, then re-encoded with the Rust encoder).
#        d. Assert A == B (byte-identical: narrowest-encoding rule means two
#           SPEC-compliant encoders must agree).
#        e. Decode A with `Kobako::Codec::Decoder` -> recovered_a; assert
#           recovered_a == original.
#        f. Decode B with `Kobako::Codec::Decoder` -> recovered_b; assert
#           recovered_b == original (covers Rust-encoded -> Ruby-decoded).
#
# Configuration:
#   KOBAKO_FUZZ_ITERATIONS=N  (default 1000)
#   KOBAKO_FUZZ_SEED=N        (default: random; printed for reproduction)
#   KOBAKO_FUZZ_HEAVY=1       (bumps to 100_000 — nightly tier)
#
# An absent Rust toolchain is handled by +GuestGuard#require_cargo_oracle!+,
# which owns the local-skip versus CI-failure split.

require "test_helper"

class TestCodecRoundtripFuzz < Minitest::Test
  include GuestGuard

  CRATE_DIR = TestPaths.source("wasm", "kobako-wasm")
  ORACLE    = CargoOracle.new(crate_dir: CRATE_DIR, bin_name: "roundtrip_oracle")

  Encoder = Kobako::Codec::Encoder
  Decoder = Kobako::Codec::Decoder

  def setup
    require_cargo_oracle!(ORACLE)
    initialize_fuzzer_params
  end

  def test_round_trip_fuzz
    ORACLE.open do |channel|
      @iterations.times do |i|
        run_one(@generator.generate, i, channel)
      end
    end
    assert_coverage_complete
  end

  private

  def initialize_fuzzer_params
    @iterations = (ENV["KOBAKO_FUZZ_ITERATIONS"] || "1000").to_i
    @iterations = 100_000 if ENV["KOBAKO_FUZZ_HEAVY"] == "1"
    @seed = (ENV["KOBAKO_FUZZ_SEED"] || Random.new_seed.to_s).to_i
    @generator = WireValueGenerator.new(rng: Random.new(@seed))
  end

  def assert_coverage_complete
    coverage = @generator.coverage
    missing = @generator.coverage_keys.reject { |k| coverage[k].positive? }
    msg = "fuzz coverage gap (seed=#{@seed}): #{missing.inspect}; counters=#{coverage.inspect}"
    assert missing.empty?, msg
  end

  def run_one(value, iter, process)
    encoded_a = Encoder.encode(value)
    encoded_b = exchange_frame(process, iter, value, encoded_a)
    assert_byte_identical_encodings(iter, value, encoded_a, encoded_b)
    assert_ruby_roundtrip(iter, value, encoded_a, "Ruby encode -> Ruby decode mismatch")
    assert_ruby_roundtrip(iter, value, encoded_b, "Ruby encode -> Rust re-encode -> Ruby decode mismatch")
  end

  def exchange_frame(process, iter, value, encoded_a)
    process.send_frame(encoded_a)
    body, error = process.read_frame
    flunk_oracle_error(iter, value, body) if error
    body
  rescue IOError => e
    flunk fuzz_failure(iter, value, e.message)
  end

  def assert_byte_identical_encodings(iter, value, encoded_a, encoded_b)
    return if encoded_a == encoded_b

    flunk fuzz_failure(iter, value, "Rust re-encoded bytes differ from Ruby-encoded bytes",
                       ruby_bytes: encoded_a, rust_bytes: encoded_b)
  end

  def assert_ruby_roundtrip(iter, value, encoded, message)
    recovered = Decoder.decode(encoded)
    failure_msg = fuzz_failure(iter, value, message, decoded: recovered)
    if value.nil?
      assert_nil recovered, failure_msg
    else
      assert_equal value, recovered, failure_msg
    end
  end

  def flunk_oracle_error(iter, value, payload)
    tag = payload.byteslice(0, 1)
    msg = payload.byteslice(1, payload.bytesize - 1)
    flunk fuzz_failure(iter, value, "oracle reported wire error tag=#{tag.inspect} msg=#{msg.inspect}")
  end

  def fuzz_failure(iter, value, msg, **extra)
    parts = [
      "fuzz failure on iteration #{iter} (seed=#{@seed})",
      "  message: #{msg}",
      "  value:   #{value.inspect[0, 200]}"
    ]
    extra.each { |k, v| parts << "  #{k}: #{format_extra_value(v)}" }
    parts.join("\n")
  end

  def format_extra_value(value)
    return value.unpack1("H*")[0, 200] if value.is_a?(String) && value.encoding == Encoding::ASCII_8BIT

    value.inspect[0, 200]
  end
end
