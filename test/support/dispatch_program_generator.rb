# frozen_string_literal: true

# Seeded generator of random Handle-minting mruby dispatch programs, used by
# the concurrency scheduling fuzz (test/fuzz/test_dispatch_scheduling_fuzz.rb)
# that witnesses the gvl: :release scheduling-only guarantee (SPEC.md B-64).
#
# Every program evaluates to an Array of one leaf per element. Each element
# mints exactly one Capability Handle from a bound Vault::Mint (which returns
# a host Token tagged with the Sandbox's owner) and routes that Handle back
# through one randomly chosen path before its owner is observed. The way the
# Handle travels — as a dispatch argument, through a guest container, out of
# a yielded block, through nested guest->host dispatch, or returned bare for
# host-side restoration — is what varies the shape exercised under the GVL
# re-entry; the owner every path yields is always the minting Sandbox's tag.
#
# +generate+ returns the program plus a per-element +:int+ / +:token+ kind
# list, so the harness predicts the canonical result for any tag without
# parsing the program: an :int leaf reads the owner host-side (B-16), a
# :token leaf returns the Handle for host-side restoration (B-37).
class DispatchProgramGenerator
  MAX_ELEMENTS = 8
  MAX_NEST = 3

  # Variants whose leaf reads the owner integer host-side (B-16 argument
  # resolution), each travelling the Handle a different way first.
  OWNER_VARIANTS = %i[arg_direct via_local via_array via_hash yield_break nested_dispatch].freeze
  # Variants whose leaf returns the Handle itself for host-side restoration
  # (B-37).
  RETURN_VARIANTS = %i[return_bare].freeze
  VARIANTS = (OWNER_VARIANTS + RETURN_VARIANTS).freeze

  # Every key here must be observed at least once in a run, so a regression
  # that quietly stops emitting a path or a size band fails the harness
  # independently of any parity mismatch.
  COVERAGE_KEYS = (VARIANTS + %i[deep_nesting single_element many_elements]).freeze

  # The static owner-read / return paths as +[expression, kind]+ pairs;
  # +nested_dispatch+ is the one variant whose depth is randomized, so it is
  # emitted separately.
  STATIC_EMITTERS = {
    arg_direct: ["Vault::Owner.call(Vault::Mint.call)", :int],
    via_local: ["(t = Vault::Mint.call; Vault::Owner.call(t))", :int],
    via_array: ["Vault::Owner.call([Vault::Mint.call].first)", :int],
    via_hash: ["Vault::Owner.call({ k: Vault::Mint.call }[:k])", :int],
    yield_break: ["Vault::Each.call([0]) { |_| break Vault::Owner.call(Vault::Mint.call) }", :int],
    return_bare: ["Vault::Mint.call", :token]
  }.freeze

  attr_reader :coverage

  def initialize(rng:)
    @rng = rng
    @coverage = Hash.new(0)
  end

  # Returns +[program, kinds]+ where +kinds+ is one +:int+ / +:token+ per
  # top-level element; updates +coverage+ as a side effect.
  def generate
    count = @rng.rand(1..MAX_ELEMENTS)
    @coverage[count == 1 ? :single_element : :many_elements] += 1
    pairs = Array.new(count) { emit(pick_variant) }
    ["[#{pairs.map(&:first).join(", ")}]", pairs.map(&:last)]
  end

  private

  def pick_variant
    variant = VARIANTS.sample(random: @rng)
    @coverage[variant] += 1
    variant
  end

  def emit(variant)
    return [emit_nested, :int] if variant == :nested_dispatch

    STATIC_EMITTERS.fetch(variant)
  end

  # Wrap the owner read in 1..MAX_NEST Vault::Wrap yield frames, exercising
  # nested guest->host dispatch (B-28) under the GVL re-entry to varying
  # depth.
  def emit_nested
    depth = @rng.rand(1..MAX_NEST)
    @coverage[:deep_nesting] += 1 if depth >= 2
    expr = "Vault::Owner.call(Vault::Mint.call)"
    depth.times { expr = "Vault::Wrap.call { #{expr} }" }
    expr
  end
end
