# frozen_string_literal: true

# E2E round-trip fuzz harness for the kobako wire codec (SPEC item #7).
#
# This is THE proof that the two independent codec implementations (the
# pure-Ruby `Kobako::Wire` codec under lib/kobako/wire and the hand-written
# Rust codec under wasm/kobako-wasm/src/codec) agree on the wire. SPEC.md
# pins round-trip fuzz as the *sole* consistency mechanism between the two
# implementations — there is no shared codec source — so a passing fuzz run
# is the release gate per SPEC's "Release Blockers" table (item #1).
#
# Architecture (per cycle 7 handoff):
#
#   1. Build the Rust oracle binary `roundtrip_oracle` once at test start
#      (cargo build --release).
#   2. Spawn it as a long-lived subprocess via IO.popen and stream
#      length-prefixed frames over stdin/stdout. Per-iteration cargo run
#      would dominate the wall-clock budget.
#   3. For each iteration:
#        a. Generate a random Ruby value with a seeded RNG.
#        b. Encode with `Kobako::Wire::Codec::Encoder` -> bytes A.
#        c. Send bytes A to the oracle; receive bytes B (oracle decoded with
#           the Rust codec, then re-encoded with the Rust encoder).
#        d. Assert A == B (byte-identical: narrowest-encoding rule means two
#           SPEC-compliant encoders must agree).
#        e. Decode A with `Kobako::Wire::Codec::Decoder` -> recovered_a; assert
#           recovered_a == original.
#        f. Decode B with `Kobako::Wire::Codec::Decoder` -> recovered_b; assert
#           recovered_b == original (covers Rust-encoded -> Ruby-decoded).
#
# Configuration:
#   KOBAKO_FUZZ_ITERATIONS=N  (default 1000)
#   KOBAKO_FUZZ_SEED=N        (default: random; printed for reproduction)
#   KOBAKO_FUZZ_HEAVY=1       (bumps to 100_000 — nightly tier)
#
# Skip semantics:
#   * If `cargo` is not on PATH: skip with informative message (consistent
#     with the cycle-5 pattern in test_wasm_crate.rb).

require "minitest/autorun"
require_relative "support/cargo_oracle"
require_relative "support/wire_value_generator"

$LOAD_PATH.unshift File.expand_path("../lib", __dir__)
require "kobako/wire"

class TestCodecRoundtripFuzz < Minitest::Test
  CRATE_DIR = File.expand_path("../wasm/kobako-wasm", __dir__)
  ORACLE    = CargoOracle.new(crate_dir: CRATE_DIR, bin_name: "roundtrip_oracle")

  Encoder = Kobako::Wire::Codec::Encoder
  Decoder = Kobako::Wire::Codec::Decoder

  def setup
    check_oracle_status
    initialize_fuzzer_params
  end

  def test_round_trip_fuzz
    ORACLE.open do |channel|
      @iterations.times do |i|
        run_one(@generator.generate, i, channel)
      end
    end
    assert_coverage_complete
    puts "\nfuzz coverage (seed=#{@seed}, iterations=#{@iterations}): #{@generator.coverage.inspect}"
  end

  private

  def check_oracle_status
    case (build = ORACLE.ensure_built).status
    when :no_cargo
      skip "cargo not on PATH — skipping codec round-trip fuzz (install rustup to enable)"
    when :build_failed
      flunk "cargo build --release roundtrip_oracle failed:\n#{build.error}"
    end
  end

  def initialize_fuzzer_params
    @iterations = (ENV["KOBAKO_FUZZ_ITERATIONS"] || "1000").to_i
    @iterations = 100_000 if ENV["KOBAKO_FUZZ_HEAVY"] == "1"
    @seed = (ENV["KOBAKO_FUZZ_SEED"] || Random.new_seed.to_s).to_i
    @generator = WireValueGenerator.new(rng: Random.new(@seed))
  end

  def assert_coverage_complete
    coverage = @generator.coverage
    missing = WireValueGenerator::COVERAGE_KEYS.reject { |k| coverage[k].positive? }
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
    return if values_equal?(value, recovered)

    flunk fuzz_failure(iter, value, message, decoded: recovered)
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

  # ---------------------------------------------------------------------
  # Value comparison
  #
  # NaN edge case: per IEEE 754, NaN != NaN. We compare floats with
  # NaN-awareness so that round-tripping a NaN counts as success. SPEC.md
  # does not constrain NaN bit patterns; the Rust codec always emits f64,
  # so we don't need to compare bit patterns either.
  # ---------------------------------------------------------------------

  def values_equal?(lhs, rhs)
    return float_equal?(lhs, rhs) if lhs.is_a?(Float) && rhs.is_a?(Float)
    return array_equal?(lhs, rhs) if lhs.is_a?(Array) && rhs.is_a?(Array)
    return hash_equal?(lhs, rhs) if lhs.is_a?(Hash) && rhs.is_a?(Hash)

    lhs == rhs
  end

  def float_equal?(lhs, rhs)
    return true if lhs.nan? && rhs.nan?

    lhs == rhs
  end

  def array_equal?(lhs, rhs)
    return false unless lhs.length == rhs.length

    lhs.each_with_index.all? { |element, i| values_equal?(element, rhs[i]) }
  end

  def hash_equal?(lhs, rhs)
    return false unless lhs.size == rhs.size

    lhs.all? { |k, v| rhs.key?(k) && values_equal?(v, rhs[k]) }
  end
end
