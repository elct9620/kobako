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
#        b. Encode with `Kobako::Wire::Encoder` -> bytes A.
#        c. Send bytes A to the oracle; receive bytes B (oracle decoded with
#           the Rust codec, then re-encoded with the Rust encoder).
#        d. Assert A == B (byte-identical: narrowest-encoding rule means two
#           SPEC-compliant encoders must agree).
#        e. Decode A with `Kobako::Wire::Decoder` -> recovered_a; assert
#           recovered_a == original.
#        f. Decode B with `Kobako::Wire::Decoder` -> recovered_b; assert
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
require "open3"

$LOAD_PATH.unshift File.expand_path("../lib", __dir__)
require "kobako/wire"

class TestCodecRoundtripFuzz < Minitest::Test
  PROJECT_ROOT  = File.expand_path("..", __dir__)
  CRATE_DIR     = File.join(PROJECT_ROOT, "wasm", "kobako-wasm")
  ORACLE_BIN    = File.join(CRATE_DIR, "target", "release", "roundtrip_oracle")
  CARGO_MANIFEST = File.join(CRATE_DIR, "Cargo.toml")

  Encoder  = Kobako::Wire::Encoder
  Decoder  = Kobako::Wire::Decoder
  Handle   = Kobako::Wire::Handle
  Exc      = Kobako::Wire::Exception

  ERROR_FLAG = 0x8000_0000

  # ---------------------------------------------------------------------
  # One-time oracle build + spawn (memoised across the suite).
  # ---------------------------------------------------------------------

  @@oracle_build_status = nil # :ok / :no_cargo / :build_failed
  @@oracle_build_error  = nil

  def self.ensure_oracle_built
    return @@oracle_build_status if @@oracle_build_status

    unless system("command -v cargo > /dev/null 2>&1")
      @@oracle_build_status = :no_cargo
      return @@oracle_build_status
    end

    out, status = Open3.capture2e(
      "cargo", "build", "--release",
      "--manifest-path", CARGO_MANIFEST,
      "--bin", "roundtrip_oracle"
    )
    if status.success? && File.executable?(ORACLE_BIN)
      @@oracle_build_status = :ok
    else
      @@oracle_build_status = :build_failed
      @@oracle_build_error = out
    end
    @@oracle_build_status
  end

  def setup
    case self.class.ensure_oracle_built
    when :no_cargo
      skip "cargo not on PATH — skipping codec round-trip fuzz (install rustup to enable)"
    when :build_failed
      flunk "cargo build --release roundtrip_oracle failed:\n#{@@oracle_build_error}"
    end

    @iterations = (ENV["KOBAKO_FUZZ_ITERATIONS"] || "1000").to_i
    @iterations = 100_000 if ENV["KOBAKO_FUZZ_HEAVY"] == "1"
    @seed = (ENV["KOBAKO_FUZZ_SEED"] || Random.new_seed.to_s).to_i
    @rng = Random.new(@seed)
    @coverage = Hash.new(0)
  end

  # ---------------------------------------------------------------------
  # The fuzz test itself.
  # ---------------------------------------------------------------------

  def test_round_trip_fuzz
    stdin, stdout, wait_thr = Open3.popen2(ORACLE_BIN)
    stdin.binmode
    stdout.binmode

    begin
      @iterations.times do |i|
        value = generate_value(depth: 0)
        run_one(value, i, stdin, stdout)
      end
    ensure
      stdin.close unless stdin.closed?
      # Drain stdout so the child can exit cleanly.
      begin
        stdout.read
      rescue StandardError
        # ignore — we only care about clean shutdown
      end
      stdout.close unless stdout.closed?
      wait_thr.join
    end

    # Coverage report: each of the 11 wire types and both ext types must
    # have been visited at least once across the run. Boundary lengths get
    # their own counters so a regression that stops generating large
    # str/bin/array/map values is caught.
    required = %i[
      nil bool int_pos_fix int_neg_fix int_u8 int_u16 int_u32 int_u64
      int_i8 int_i16 int_i32 int_i64 float str_empty str_fix str_8 str_16
      bin_empty bin_8 bin_16 array_empty array_fix array_16 map_empty
      map_fix map_16 handle exception nesting
    ]
    missing = required.reject { |k| @coverage[k].positive? }
    assert missing.empty?,
           "fuzz coverage gap (seed=#{@seed}): #{missing.inspect}; " \
           "counters=#{@coverage.inspect}"

    puts "\nfuzz coverage (seed=#{@seed}, iterations=#{@iterations}): #{@coverage.inspect}"
  end

  private

  def run_one(value, iter, stdin, stdout)
    encoded_a = Encoder.encode(value)
    write_frame(stdin, encoded_a)
    stdin.flush
    encoded_b = read_frame(stdout, iter, value)

    unless encoded_a == encoded_b
      flunk fuzz_failure(
        iter, value,
        "Rust re-encoded bytes differ from Ruby-encoded bytes",
        ruby_bytes: encoded_a, rust_bytes: encoded_b
      )
    end

    recovered_a = Decoder.new(encoded_a).read
    unless values_equal?(value, recovered_a)
      flunk fuzz_failure(iter, value, "Ruby encode -> Ruby decode mismatch",
                         decoded: recovered_a)
    end

    recovered_b = Decoder.new(encoded_b).read
    return if values_equal?(value, recovered_b)

    flunk fuzz_failure(iter, value, "Ruby encode -> Rust re-encode -> Ruby decode mismatch",
                       decoded: recovered_b)
  end

  def write_frame(io, bytes)
    io.write([bytes.bytesize].pack("N"))
    io.write(bytes)
  end

  def read_frame(io, iter, value)
    hdr = io.read(4)
    flunk fuzz_failure(iter, value, "oracle stdout closed unexpectedly (no header)") if hdr.nil? || hdr.bytesize < 4
    word = hdr.unpack1("N")
    is_error = word.anybits?(ERROR_FLAG)
    len = word & ~ERROR_FLAG
    payload = len.zero? ? "".b : io.read(len)
    if payload.nil? || payload.bytesize < len
      flunk fuzz_failure(iter, value, "oracle stdout truncated (header said #{len} bytes)")
    end

    if is_error
      tag = payload.byteslice(0, 1)
      msg = payload.byteslice(1, payload.bytesize - 1)
      flunk fuzz_failure(iter, value, "oracle reported wire error tag=#{tag.inspect} msg=#{msg.inspect}")
    end

    payload.b
  end

  def fuzz_failure(iter, value, msg, **extra)
    parts = [
      "fuzz failure on iteration #{iter} (seed=#{@seed})",
      "  message: #{msg}",
      "  value:   #{value.inspect[0, 200]}"
    ]
    extra.each do |k, v|
      summary =
        if v.is_a?(String) && v.encoding == Encoding::ASCII_8BIT
          v.unpack1("H*")[0, 200]
        else
          v.inspect[0, 200]
        end
      parts << "  #{k}: #{summary}"
    end
    parts.join("\n")
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
  # Hand-rolled, seeded. Produces values across all 11 wire types plus
  # both ext types; every msgpack length-class boundary (fixstr/str8/str16,
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
    case @rng.rand(8)
    when 0 then track(:nil) { nil }
    when 1 then track(:bool) { @rng.rand(2).zero? }
    when 2 then generate_integer
    when 3 then generate_float
    when 4, 5 then generate_string
    when 6 then generate_binary
    end
  end

  # ----- integers -----

  INT_BANDS = %i[
    pos_fix neg_fix u8 u16 u32 u64 i8 i16 i32 i64
  ].freeze

  def generate_integer
    band = INT_BANDS.sample(random: @rng)
    n = case band
        when :pos_fix then @rng.rand(0..0x7f)
        when :neg_fix then -@rng.rand(1..32)
        when :u8      then @rng.rand(0x80..0xff)
        when :u16     then @rng.rand(0x100..0xffff)
        when :u32     then @rng.rand(0x1_0000..0xffff_ffff)
        when :u64     then @rng.rand(0x1_0000_0000..0xffff_ffff_ffff_ffff)
        when :i8      then -@rng.rand(33..0x80)
        when :i16     then -@rng.rand(0x81..0x8000)
        when :i32     then -@rng.rand(0x8001..0x8000_0000)
        when :i64     then -@rng.rand(0x8000_0001..0x8000_0000_0000_0000)
        end
    @coverage[:"int_#{band}"] += 1
    n
  end

  # ----- floats -----

  def generate_float
    track(:float) do
      pick = @rng.rand(10)
      case pick
      when 0 then 0.0
      when 1 then -0.0
      when 2 then Float::INFINITY
      when 3 then -Float::INFINITY
      when 4 then 1.0
      when 5 then -1.0
      else
        # General floats; deliberately skip NaN since the comparator
        # already handles it but generating NaN here gives the byte-equality
        # check (encoded_a == encoded_b) trouble across NaN bit patterns
        # produced by f64::from_bits round-trip.
        sign = @rng.rand(2).zero? ? 1.0 : -1.0
        mantissa = @rng.rand
        exponent = @rng.rand(-50..50)
        sign * mantissa * (10.0**exponent)
      end
    end
  end

  # ----- strings -----

  def generate_string
    pick = @rng.rand(20)
    if pick.zero?
      @coverage[:str_empty] += 1
      ""
    elsif pick < 12
      # fixstr: 1..31
      len = @rng.rand(1..31)
      @coverage[:str_fix] += 1
      random_utf8_string(len)
    elsif pick < 17
      # str 8: 32..255
      len = @rng.rand(32..255)
      @coverage[:str_8] += 1
      random_utf8_string(len)
    else
      # str 16: 256..2048 (we cap below the full u16 range to keep the
      # fuzz cheap; SPEC's narrowest-encoding rule still gets exercised).
      len = @rng.rand(256..2048)
      @coverage[:str_16] += 1
      random_utf8_string(len)
    end
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

  def generate_binary
    pick = @rng.rand(20)
    if pick.zero?
      @coverage[:bin_empty] += 1
      "".b
    elsif pick < 14
      # bin 8: 1..255
      len = @rng.rand(1..255)
      @coverage[:bin_8] += 1
      random_bytes(len)
    else
      # bin 16: 256..2048
      len = @rng.rand(256..2048)
      @coverage[:bin_16] += 1
      random_bytes(len)
    end
  end

  def random_bytes(n)
    Array.new(n) { @rng.rand(0..255) }.pack("C*")
  end

  # ----- containers -----

  def generate_array(depth:)
    pick = @rng.rand(20)
    len =
      if pick.zero?
        @coverage[:array_empty] += 1
        0
      elsif pick < 16
        @coverage[:array_fix] += 1
        @rng.rand(1..15)
      else
        @coverage[:array_16] += 1
        @rng.rand(16..32) # cap small so deep nesting doesn't explode payload size
      end
    @coverage[:nesting] += 1 if depth > 1
    Array.new(len) { generate_value(depth: depth) }
  end

  def generate_map(depth:)
    pick = @rng.rand(20)
    len =
      if pick.zero?
        @coverage[:map_empty] += 1
        0
      elsif pick < 16
        @coverage[:map_fix] += 1
        @rng.rand(1..15)
      else
        @coverage[:map_16] += 1
        @rng.rand(16..24)
      end
    h = {}
    # Use unique scalar keys to avoid accidental collisions that would
    # shrink the map and skew the boundary coverage.
    h[generate_map_key] = generate_value(depth: depth) while h.size < len
    @coverage[:nesting] += 1 if depth > 1
    h
  end

  def generate_map_key
    case @rng.rand(4)
    when 0 then random_utf8_string(@rng.rand(1..16))
    when 1 then @rng.rand(-1000..1000)
    when 2 then @rng.rand(2).zero?
    else "k#{@rng.rand(1_000_000)}"
    end
  end

  # ----- ext types -----

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
