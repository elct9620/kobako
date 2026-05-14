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

$LOAD_PATH.unshift File.expand_path("../lib", __dir__)
require "kobako/wire"

class TestCodecRoundtripFuzz < Minitest::Test
  CRATE_DIR = File.expand_path("../wasm/kobako-wasm", __dir__)
  ORACLE    = CargoOracle.new(crate_dir: CRATE_DIR, bin_name: "roundtrip_oracle")

  Encoder = Kobako::Wire::Codec::Encoder
  Decoder = Kobako::Wire::Codec::Decoder
  Handle  = Kobako::Wire::Handle
  Exc     = Kobako::Wire::Exception

  def setup
    check_oracle_status
    initialize_fuzzer_params
  end

  # Coverage report: each of the 12 wire types and all three ext types
  # must have been visited at least once across the run. Boundary
  # lengths get their own counters so a regression that stops generating
  # large str/bin/array/map values is caught.
  REQUIRED_COVERAGE_KEYS = %i[
    nil bool int_pos_fix int_neg_fix int_u8 int_u16 int_u32 int_u64
    int_i8 int_i16 int_i32 int_i64 float str_empty str_fix str_8 str_16
    bin_empty bin_8 bin_16 array_empty array_fix array_16 map_empty
    map_fix map_16 symbol handle exception nesting
  ].freeze

  def test_round_trip_fuzz
    ORACLE.open do |process|
      @iterations.times do |i|
        run_one(generate_value(depth: 0), i, process)
      end
    end
    assert_coverage_complete
    puts "\nfuzz coverage (seed=#{@seed}, iterations=#{@iterations}): #{@coverage.inspect}"
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
    @rng = Random.new(@seed)
    @coverage = Hash.new(0)
  end

  def assert_coverage_complete
    missing = REQUIRED_COVERAGE_KEYS.reject { |k| @coverage[k].positive? }
    msg = "fuzz coverage gap (seed=#{@seed}): #{missing.inspect}; counters=#{@coverage.inspect}"
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

  def values_equal?(a, b)
    return float_equal?(a, b) if a.is_a?(Float) && b.is_a?(Float)
    return array_equal?(a, b) if a.is_a?(Array) && b.is_a?(Array)
    return hash_equal?(a, b) if a.is_a?(Hash) && b.is_a?(Hash)

    a == b
  end

  def float_equal?(a, b)
    return true if a.nan? && b.nan?

    a == b
  end

  def array_equal?(a, b)
    return false unless a.length == b.length

    a.each_with_index.all? { |x, i| values_equal?(x, b[i]) }
  end

  def hash_equal?(a, b)
    return false unless a.size == b.size

    a.all? { |k, v| b.key?(k) && values_equal?(v, b[k]) }
  end

  # ---------------------------------------------------------------------
  # Random value generator
  #
  # Hand-rolled, seeded. Produces values across all 12 wire types plus
  # all three ext types; every msgpack length-class boundary (fixstr/str8/str16,
  # bin8/bin16, fixarray/array16, fixmap/map16) is given non-trivial
  # probability so coverage is hit reliably within 1000 iterations.
  # ---------------------------------------------------------------------

  MAX_DEPTH = 4

  def generate_value(depth:)
    return generate_scalar if depth >= MAX_DEPTH

    # Bias toward scalars so tests don't blow up from runaway recursion;
    # containers still get plenty of coverage at depth 0/1/2.
    bucket = @rng.rand(100)
    case bucket
    when 0..69 then generate_scalar
    when 70..82 then generate_array(depth: depth + 1)
    when 83..94 then generate_map(depth: depth + 1)
    when 95..96 then generate_handle
    when 97..99 then generate_exception
    end
  end

  def generate_scalar
    case @rng.rand(9)
    when 0 then track(:nil) { nil }
    when 1 then track(:bool) { @rng.rand(2).zero? }
    when 2 then generate_integer
    when 3 then generate_float
    when 4, 5 then generate_string
    when 6 then generate_binary
    when 7 then generate_symbol
    end
  end

  # ----- integers -----

  # Each band maps to a lambda that samples its msgpack-format range from
  # the shared RNG. Negative bands keep the `-rng.rand(positive_range)`
  # form to preserve the seeded value sequence — switching to a direct
  # negative range would consume the same number of RNG draws but yield
  # different values for the same seed.
  INT_BAND_SAMPLERS = {
    pos_fix: ->(rng) { rng.rand(0..0x7f) },
    neg_fix: ->(rng) { -rng.rand(1..32) },
    u8: ->(rng) { rng.rand(0x80..0xff) },
    u16: ->(rng) { rng.rand(0x100..0xffff) },
    u32: ->(rng) { rng.rand(0x1_0000..0xffff_ffff) },
    u64: ->(rng) { rng.rand(0x1_0000_0000..0xffff_ffff_ffff_ffff) },
    i8: ->(rng) { -rng.rand(33..0x80) },
    i16: ->(rng) { -rng.rand(0x81..0x8000) },
    i32: ->(rng) { -rng.rand(0x8001..0x8000_0000) },
    i64: ->(rng) { -rng.rand(0x8000_0001..0x8000_0000_0000_0000) }
  }.freeze
  INT_BANDS = INT_BAND_SAMPLERS.keys.freeze

  def generate_integer
    band = INT_BANDS.sample(random: @rng)
    @coverage[:"int_#{band}"] += 1
    INT_BAND_SAMPLERS.fetch(band).call(@rng)
  end

  # ----- floats -----

  # Special-value floats indexed 0..5; bucket 6..9 falls through to
  # random_general_float. SPEC.md does not constrain NaN bit patterns;
  # NaN is deliberately omitted because f64::from_bits round-trip can
  # mutate the bit pattern and break the byte-equality check.
  SPECIAL_FLOATS = [0.0, -0.0, Float::INFINITY, -Float::INFINITY, 1.0, -1.0].freeze

  def generate_float
    track(:float) do
      pick = @rng.rand(10)
      pick < SPECIAL_FLOATS.size ? SPECIAL_FLOATS[pick] : random_general_float
    end
  end

  def random_general_float
    sign = @rng.rand(2).zero? ? 1.0 : -1.0
    mantissa = @rng.rand
    exponent = @rng.rand(-50..50)
    sign * mantissa * (10.0**exponent)
  end

  # ----- strings -----

  # str-family msgpack length bands: each entry is [upper_bound_exclusive
  # in the 20-bucket pick, coverage key, byte-length range or nil for
  # the empty-string fast path]. We cap str_16 below the full u16 range
  # to keep the fuzz cheap; SPEC's narrowest-encoding rule still gets
  # exercised at the band boundaries.
  STRING_BANDS = [
    [1,  :str_empty, nil],
    [12, :str_fix,   1..31],
    [17, :str_8,     32..255],
    [20, :str_16,    256..2048]
  ].freeze

  def generate_string
    pick = @rng.rand(20)
    _, key, range = STRING_BANDS.find { |upper, *| pick < upper }
    @coverage[key] += 1
    range ? random_utf8_string(@rng.rand(range)) : ""
  end

  ASCII_PRINTABLE = (32..126).to_a.freeze

  def random_utf8_string(byte_len)
    # Build an ASCII string of exactly +byte_len+ bytes. ASCII is a UTF-8
    # subset, so the bytesize == char count. For multibyte coverage we
    # occasionally splice in a small UTF-8 token, then re-trim to the
    # requested length.
    s = String.new(encoding: Encoding::UTF_8)
    s << ASCII_PRINTABLE.sample(random: @rng).chr(Encoding::UTF_8) while s.bytesize < byte_len

    # Multibyte sprinkle: 25% of the time, replace a tail slice with
    # multibyte chars (only when there's room).
    if byte_len >= 6 && @rng.rand(4).zero?
      candidates = ["蒼", "時", "弦", "也", "🌸", "λ", "Ω", "α"]
      pick = candidates.sample(random: @rng)
      # Replace the last `pick.bytesize` bytes with the multibyte char,
      # preserving the total byte length.
      cut = pick.bytesize
      s = s.byteslice(0, s.bytesize - cut).force_encoding(Encoding::UTF_8) + pick if s.bytesize > cut
    end
    s.force_encoding(Encoding::UTF_8)
  end

  # ----- binary -----

  # bin-family msgpack length bands (same shape as STRING_BANDS).
  BINARY_BANDS = [
    [1,  :bin_empty, nil],
    [14, :bin_8,     1..255],
    [20, :bin_16,    256..2048]
  ].freeze

  def generate_binary
    pick = @rng.rand(20)
    _, key, range = BINARY_BANDS.find { |upper, *| pick < upper }
    @coverage[key] += 1
    range ? random_bytes(@rng.rand(range)) : "".b
  end

  def random_bytes(n)
    Array.new(n) { @rng.rand(0..255) }.pack("C*")
  end

  # ----- containers -----

  # Container msgpack length bands. array_16 is capped small so deep
  # nesting doesn't explode payload size.
  ARRAY_BANDS = [
    [1,  :array_empty, nil],
    [16, :array_fix,   1..15],
    [20, :array_16,    16..32]
  ].freeze

  MAP_BANDS = [
    [1,  :map_empty, nil],
    [16, :map_fix,   1..15],
    [20, :map_16,    16..24]
  ].freeze

  def generate_array(depth:)
    len = pick_container_length(ARRAY_BANDS)
    @coverage[:nesting] += 1 if depth > 1
    Array.new(len) { generate_value(depth: depth) }
  end

  def generate_map(depth:)
    len = pick_container_length(MAP_BANDS)
    h = {}
    # Use unique scalar keys to avoid accidental collisions that would
    # shrink the map and skew the boundary coverage.
    h[generate_map_key] = generate_value(depth: depth) while h.size < len
    @coverage[:nesting] += 1 if depth > 1
    h
  end

  def pick_container_length(bands)
    pick = @rng.rand(20)
    _, key, range = bands.find { |upper, *| pick < upper }
    @coverage[key] += 1
    range ? @rng.rand(range) : 0
  end

  def generate_map_key
    case @rng.rand(5)
    when 0 then random_utf8_string(@rng.rand(1..16))
    when 1 then @rng.rand(-1000..1000)
    when 2 then @rng.rand(2).zero?
    when 3 then generate_symbol
    else "k#{@rng.rand(1_000_000)}"
    end
  end

  # ----- ext types -----

  # SPEC.md → Wire Codec → Ext Types → ext 0x00: Symbol payload is UTF-8
  # bytes; empty payload is wire-legal. ~5% empty + 95% random 1..64-byte
  # UTF-8 names — the random range crosses the fixext1 / 2 / 4 / 8 / 16
  # and ext 8 boundaries automatically, which is the minimum needed to
  # catch a regression that breaks one of those framing tiers. The Rust
  # guest's symbol unit tests cover the ext 16 boundary separately.
  def generate_symbol
    @coverage[:symbol] += 1
    pick = @rng.rand(20)
    name =
      if pick.zero?
        ""
      else
        random_utf8_string(@rng.rand(1..64))
      end
    name.to_sym
  end

  def generate_handle
    @coverage[:handle] += 1
    Handle.new(@rng.rand(Handle::MIN_ID..Handle::MAX_ID))
  end

  EXC_TYPES = %w[runtime argument disconnected undefined].freeze

  def generate_exception
    @coverage[:exception] += 1
    type    = EXC_TYPES.sample(random: @rng)
    message = random_utf8_string(@rng.rand(1..40))
    details =
      case @rng.rand(3)
      when 0 then nil
      when 1 then random_utf8_string(@rng.rand(1..32))
      else        { "field" => random_utf8_string(@rng.rand(1..16)) }
      end
    Exc.new(type: type, message: message, details: details)
  end

  def track(key)
    @coverage[key] += 1
    yield
  end
end
