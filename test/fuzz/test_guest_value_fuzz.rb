# frozen_string_literal: true

require "test_helper"

# Fuzz (drives real data/kobako.wasm) — value fidelity across the guest's
# own conversion walk (wasm/kobako-mruby/src/msgpack/convert.rs).
#
# The payload layer carries three implementations; the two the round-trip
# harness differs against each other are both host-side, and this walk is
# the third. It has no peer to compare bytes with, so its oracle is an
# identity law instead: a value the host puts on the wire comes back the
# value it went in as.
#
# One #run crosses every payload position the walk owns, because the
# entrypoint hands its argument to a bound Service and returns what the
# Service answered:
#
#   host --Run payload--> guest --Call args--> Service
#   host <---Outcome----- guest <--Reply ok--- Service
#
# so the value the Service was handed tells the two directions apart when
# one of them loses something.
#
# The guest's value domain is narrower than the wire's in two ways the
# harness encodes rather than works around:
#
#   * The Guest Binary is built MRB_INT32, so integers outside the signed
#     32-bit range have no guest representation and are refused at the
#     boundary (E-26, covered in test/e2e/test_integer_range.rb). The
#     generator draws only bands the guest can hold.
#   * mruby Strings carry no encoding tag, so a guest re-encoding a String
#     has exactly one rule available: valid UTF-8 travels as msgpack str,
#     any other bytes as bin (docs/wire/payload-msgpack.md § Text and
#     Bytes). Both sides are re-tagged by that rule before comparing,
#     which pins the bytes the guest can lose without pinning a tag it
#     cannot know.
#
# Handles and Faults stay out of the generated domain: a Handle argument
# is restored to its host object rather than compared as a value, and a
# Fault is legal only in a Reply's fault arm.
class TestGuestValueFuzz < Minitest::Test
  include E2eGuestHelper

  # The entrypoint routes its argument out through a Service and back, so
  # one invocation crosses all four payload positions.
  ECHO_SOURCE = "class Echo; def self.call(value) = Probe::Echo.call(value); end"

  # A generated tree reaches hundreds of kilobytes once the wide bands
  # nest, which the default cap refuses. The subject here is fidelity and
  # the caps carry their own coverage (E-20), so this harness lifts the
  # one that would otherwise decide the run.
  MEMORY_LIMIT = 64 * 1024 * 1024

  def setup
    super
    initialize_fuzzer_params
  end

  # A wire value the guest can hold must survive the round trip unchanged.
  # The guest walk is the only implementation on this path, so identity is
  # the whole oracle.
  def test_guest_value_round_trip_fuzz
    sandbox = echo_sandbox
    @iterations.times { |iter| assert_round_trip(sandbox, @generator.generate, iter) }
    assert_coverage_complete
  end

  private

  def initialize_fuzzer_params
    @iterations = (ENV["KOBAKO_FUZZ_ITERATIONS"] || "1000").to_i
    @iterations = 10_000 if ENV["KOBAKO_FUZZ_HEAVY"] == "1"
    @seed = (ENV["KOBAKO_FUZZ_SEED"] || Random.new_seed.to_s).to_i
    @generator = WireValueGenerator.new(rng: Random.new(@seed),
                                        int_bands: WireValueGenerator::GUEST_INT_BANDS,
                                        ext_values: false)
  end

  def echo_sandbox
    Kobako::Sandbox.new(wasm_path: REAL_WASM, memory_limit: MEMORY_LIMIT).tap do |sandbox|
      sandbox.preload(code: ECHO_SOURCE, name: :Echo)
      sandbox.bind("Probe::Echo", ->(value) { record(value) })
    end
  end

  # Remember what the Service was handed and answer with it: the return
  # trip's input is the outbound trip's output.
  def record(value)
    @service_saw = value
  end

  def assert_round_trip(sandbox, value, iter)
    begin_iteration(value, iter)
    result = sandbox.run("Echo", value).value
    want = retag(value)
    assert_value want, retag(@service_saw),
                 "a #run argument crossing the guest to a Service must arrive unchanged"
    assert_value want, retag(result),
                 "a Service return crossing the guest must reach #run's value unchanged"
  rescue Kobako::Error => e
    flunk failure("a guest-representable value must round-trip, not raise #{e.class}: #{e.message}")
  end

  # The iteration's inputs live in ivars rather than travelling through
  # every assertion: they are what a failure has to describe, and only a
  # failure reads them.
  def begin_iteration(value, iter)
    @iteration = iter
    @generated = value
    @service_saw = nil
  end

  # The message is built lazily because assembling it walks the tree, and
  # a passing iteration must not pay for a diagnosis nobody reads.
  def assert_value(want, got, message)
    assert want == got, -> { failure(message, divergence(want, got)) }
  end

  # Re-tag every String leaf by the only rule the guest has: validity
  # decides the family. Applied to both sides, the comparison pins bytes.
  def retag(value)
    case value
    when String then value.dup.force_encoding(family_of(value))
    when Array then value.map { |element| retag(element) }
    when Hash then value.to_h { |key, val| [retag(key), retag(val)] }
    else value
    end
  end

  def family_of(string)
    string.dup.force_encoding(Encoding::UTF_8).valid_encoding? ? Encoding::UTF_8 : Encoding::BINARY
  end

  # Both directions of the narrowed domain: every declared key must be
  # reached, and nothing outside it may be produced — a leak would put a
  # value the guest cannot hold into the run and read as a guest defect.
  def assert_coverage_complete
    coverage = @generator.coverage
    declared = @generator.coverage_keys
    missing = declared.reject { |key| coverage[key].positive? }
    assert missing.empty?,
           "fuzz coverage gap (seed=#{@seed}): #{missing.inspect}; counters=#{coverage.inspect}"
    leaked = coverage.keys - declared
    assert leaked.empty?,
           "the guest domain must produce no value outside its declared keys (seed=#{@seed}): #{leaked.inspect}"
  end

  def failure(message, divergence = nil)
    parts = ["guest value fuzz failure on iteration #{@iteration} (seed=#{@seed})",
             "  message: #{message}",
             "  value:   #{@generated.inspect[0, 200]}"]
    parts << "  at:      #{divergence}" if divergence
    parts.join("\n")
  end

  # Where two trees first diverge, as an access path and the two leaves.
  # A generated value reaches kilobytes, so diffing the whole tree buries
  # the one leaf that moved; the seed already reproduces the case.
  def divergence(want, got, path = "value")
    return nil if want == got
    return container_divergence(want, got, path) if same_shape?(want, got)

    "#{path}: expected #{brief(want)}, got #{brief(got)}"
  end

  def same_shape?(want, got)
    return want.size == got.size if want.is_a?(Array) && got.is_a?(Array)

    want.is_a?(Hash) && got.is_a?(Hash) && want.keys == got.keys
  end

  def container_divergence(want, got, path)
    keys = want.is_a?(Array) ? want.each_index : want.each_key
    keys.lazy.filter_map { |key| divergence(want[key], got[key], "#{path}[#{key.inspect}]") }.first
  end

  # Strings are shown as bytes: the losses this harness hunts are byte
  # losses, and an inspected String hides them behind escapes.
  def brief(value)
    return "#{value.encoding} #{value.bytes.inspect[0, 120]}" if value.is_a?(String)

    value.inspect[0, 120]
  end
end
