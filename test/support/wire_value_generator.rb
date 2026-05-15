# frozen_string_literal: true

$LOAD_PATH.unshift File.expand_path("../../lib", __dir__)
require "kobako/rpc/envelope"

# Seeded random generator for kobako wire-encodable values, used by the
# round-trip fuzz harness (SPEC item #7).
#
# Produces values across all 12 wire types plus the three ext types, with
# explicit length bands so every msgpack format boundary
# (fixstr / str8 / str16, bin8 / bin16, fixarray / array16, fixmap / map16)
# is given non-trivial probability. The probability shape is calibrated
# so coverage of every band is hit reliably within 1000 iterations.
#
# The generator owns +rng+ and a per-instance +coverage+ counter. Each
# emitted value bumps the coverage key for the wire type and length band
# it landed in; tests assert on the counter through the +coverage+
# reader.
class WireValueGenerator
  Handle = Kobako::RPC::Handle
  Exc    = Kobako::RPC::Fault

  MAX_DEPTH = 4

  # Each band maps to a lambda that samples its msgpack-format range from
  # the shared RNG. Negative bands keep the +-rng.rand(positive_range)+
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

  # Special-value floats indexed 0..5; bucket 6..9 falls through to
  # +random_general_float+. SPEC.md does not constrain NaN bit patterns;
  # NaN is deliberately omitted because +f64::from_bits+ round-trip can
  # mutate the bit pattern and break the byte-equality check.
  SPECIAL_FLOATS = [0.0, -0.0, Float::INFINITY, -Float::INFINITY, 1.0, -1.0].freeze

  # str-family msgpack length bands: each entry is [upper_bound_exclusive
  # in the 20-bucket pick, coverage key, byte-length range or nil for
  # the empty-string fast path]. We cap +str_16+ below the full u16 range
  # to keep the fuzz cheap; SPEC's narrowest-encoding rule still gets
  # exercised at the band boundaries.
  STRING_BANDS = [
    [1,  :str_empty, nil],
    [12, :str_fix,   1..31],
    [17, :str_8,     32..255],
    [20, :str_16,    256..2048]
  ].freeze

  # bin-family msgpack length bands (same shape as STRING_BANDS).
  BINARY_BANDS = [
    [1,  :bin_empty, nil],
    [14, :bin_8,     1..255],
    [20, :bin_16,    256..2048]
  ].freeze

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

  ASCII_PRINTABLE = (32..126).to_a.freeze

  # Multibyte UTF-8 samples spliced into random ASCII strings to cover
  # the multibyte branch of the codec; kept short so they fit inside the
  # smallest str bands too.
  MULTIBYTE_SAMPLES = ["蒼", "時", "弦", "也", "🌸", "λ", "Ω", "α"].freeze

  EXC_TYPES = %w[runtime argument disconnected undefined].freeze

  # Vocabulary of coverage keys the generator may bump as a side effect.
  # Tests assert that every key in this set was visited at least once
  # during a fuzz run — a regression that quietly stops generating large
  # str/bin/array/map values or one of the ext types would otherwise
  # slip past with a green run.
  COVERAGE_KEYS = %i[
    nil bool int_pos_fix int_neg_fix int_u8 int_u16 int_u32 int_u64
    int_i8 int_i16 int_i32 int_i64 float str_empty str_fix str_8 str_16
    bin_empty bin_8 bin_16 array_empty array_fix array_16 map_empty
    map_fix map_16 symbol handle exception nesting
  ].freeze

  attr_reader :coverage

  def initialize(rng:)
    @rng = rng
    @coverage = Hash.new(0)
  end

  # Entry point. Returns a wire-encodable Ruby value bounded by
  # +MAX_DEPTH+ recursion; updates +coverage+ as a side-effect.
  def generate(depth: 0)
    return generate_scalar if depth >= MAX_DEPTH

    # Bias toward scalars so tests don't blow up from runaway recursion;
    # containers still get plenty of coverage at depth 0/1/2.
    case @rng.rand(100)
    when 0..69 then generate_scalar
    when 70..82 then generate_array(depth: depth + 1)
    when 83..94 then generate_map(depth: depth + 1)
    when 95..96 then generate_handle
    when 97..99 then generate_exception
    end
  end

  private

  # Bucket 8 has no explicit arm and falls through to +nil+ on purpose;
  # narrowing the random range would change the seed-sequence and break
  # reproducibility across the existing corpus of fuzz seeds.
  def generate_scalar
    bucket = @rng.rand(9)
    return generate_scalar_atom(bucket) if bucket <= 3

    generate_scalar_typed(bucket)
  end

  def generate_scalar_atom(bucket)
    case bucket
    when 0 then track(:nil) { nil }
    when 1 then track(:bool) { @rng.rand(2).zero? }
    when 2 then generate_integer
    when 3 then generate_float
    end
  end

  def generate_scalar_typed(bucket)
    case bucket
    when 4, 5 then generate_string
    when 6 then generate_binary
    when 7 then generate_symbol
    end
  end

  def generate_integer
    band = INT_BANDS.sample(random: @rng)
    @coverage[:"int_#{band}"] += 1
    INT_BAND_SAMPLERS.fetch(band).call(@rng)
  end

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

  def generate_string
    pick = @rng.rand(20)
    _, key, range = STRING_BANDS.find { |upper, *| pick < upper }
    @coverage[key] += 1
    range ? random_utf8_string(@rng.rand(range)) : ""
  end

  # Build an ASCII string of exactly +byte_len+ bytes (ASCII is a UTF-8
  # subset, so +bytesize+ equals char count), then optionally splice in
  # a small UTF-8 token to exercise the multibyte decoder path.
  def random_utf8_string(byte_len)
    s = String.new(encoding: Encoding::UTF_8)
    s << ASCII_PRINTABLE.sample(random: @rng).chr(Encoding::UTF_8) while s.bytesize < byte_len
    sprinkle_multibyte(s, byte_len).force_encoding(Encoding::UTF_8)
  end

  # 25% of the time, replace a tail slice with a multibyte token,
  # preserving the requested total byte length. Returns the (possibly
  # un-modified) string.
  def sprinkle_multibyte(buffer, byte_len)
    return buffer unless byte_len >= 6 && @rng.rand(4).zero?

    pick = MULTIBYTE_SAMPLES.sample(random: @rng)
    cut = pick.bytesize
    return buffer unless buffer.bytesize > cut

    buffer.byteslice(0, buffer.bytesize - cut).force_encoding(Encoding::UTF_8) + pick
  end

  def generate_binary
    pick = @rng.rand(20)
    _, key, range = BINARY_BANDS.find { |upper, *| pick < upper }
    @coverage[key] += 1
    range ? random_bytes(@rng.rand(range)) : "".b
  end

  def random_bytes(byte_count)
    Array.new(byte_count) { @rng.rand(0..255) }.pack("C*")
  end

  def generate_array(depth:)
    len = pick_container_length(ARRAY_BANDS)
    @coverage[:nesting] += 1 if depth > 1
    Array.new(len) { generate(depth: depth) }
  end

  def generate_map(depth:)
    len = pick_container_length(MAP_BANDS)
    h = {}
    # Use unique scalar keys to avoid accidental collisions that would
    # shrink the map and skew the boundary coverage.
    h[generate_map_key] = generate(depth: depth) while h.size < len
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

  # SPEC.md → Wire Codec → Ext Types → ext 0x00: Symbol payload is UTF-8
  # bytes; empty payload is wire-legal. ~5% empty + 95% random 1..64-byte
  # UTF-8 names — the random range crosses the fixext1 / 2 / 4 / 8 / 16
  # and ext 8 boundaries automatically.
  def generate_symbol
    @coverage[:symbol] += 1
    pick = @rng.rand(20)
    name = pick.zero? ? "" : random_utf8_string(@rng.rand(1..64))
    name.to_sym
  end

  def generate_handle
    @coverage[:handle] += 1
    Handle.new(@rng.rand(Handle::MIN_ID..Handle::MAX_ID))
  end

  def generate_exception
    @coverage[:exception] += 1
    type = EXC_TYPES.sample(random: @rng)
    message = random_utf8_string(@rng.rand(1..40))
    Exc.new(type: type, message: message, details: generate_exception_details)
  end

  def generate_exception_details
    case @rng.rand(3)
    when 0 then nil
    when 1 then random_utf8_string(@rng.rand(1..32))
    else        { "field" => random_utf8_string(@rng.rand(1..16)) }
    end
  end

  def track(key)
    @coverage[key] += 1
    yield
  end
end
