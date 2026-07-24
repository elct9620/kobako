# frozen_string_literal: true

require "test_helper"

# Fuzz (drives real data/kobako.wasm) — the gvl: :release scheduling-only
# guarantee over randomized Handle-minting dispatch trees (SPEC.md B-64).
# Two oracles share one seeded generator of programs that mint Capability
# Handles and route them back through varied dispatch shapes:
#
#   * Differential — the same program run under :hold and :release must
#     produce the same outcome, since releasing the GVL changes scheduling
#     only. Every dispatch in a program re-acquires the GVL under :release,
#     so this exercises the re-entry path across arbitrary shapes.
#   * Parallel isolation — distinct :release Sandboxes on distinct Threads
#     each run their own program tagged with the Thread's owner; every
#     restored owner must be that Thread's own, so a foreign owner is a
#     cross-invocation misdelivery (B-03).
#
# Fuzz discipline mirrors the Layer 1 codec harness: the seed is sourced
# from KOBAKO_FUZZ_SEED (random otherwise) and printed in every failure so
# a run reproduces from the seed, and the generator's shape coverage is
# asserted complete independently of any parity failure. Threads are per
# Sandbox because the one-Thread-per-Sandbox contract (B-22) is the
# concurrency shape SPEC sanctions today.
class TestDispatchSchedulingFuzz < Minitest::Test
  include E2eGuestHelper

  THREADS = 8

  # Per-Sandbox stateful host object; a Handle minted for it carries no wire
  # representation, so it crosses as a Capability Handle whose +owner+
  # identifies the minting Sandbox's tag.
  class Token
    attr_reader :owner

    def initialize(owner) = (@owner = owner)
  end

  def setup
    @iterations = (ENV["KOBAKO_FUZZ_ITERATIONS"] || "100").to_i
    @iterations = 1000 if ENV["KOBAKO_FUZZ_HEAVY"] == "1"
    @seed = (ENV["KOBAKO_FUZZ_SEED"] || Random.new_seed.to_s).to_i
    @generator = DispatchProgramGenerator.new(rng: Random.new(@seed))
  end

  # A generated program run under :hold and under :release must decode to
  # the same owners: releasing the GVL changes scheduling only (B-64).
  def test_release_outcome_matches_hold
    hold = tagged_sandbox(:hold, 0)
    release = tagged_sandbox(:release, 0)

    @iterations.times { |i| assert_hold_release_parity(hold, release, i) }
    assert_coverage_complete
  end

  # Distinct :release Sandboxes minting Handles on distinct Threads must each
  # resolve only their own Tokens — a foreign owner is a cross-invocation
  # misdelivery (B-03).
  def test_release_isolates_handles_across_threads
    assert_isolation_across_batches("each :release Thread must resolve only its own Handles (B-03)") do |specs|
      run_batch(specs)
    end
  end

  # Threads sharing ONE :release Sandbox, each filling Vault::Mint with its own
  # tag through the per-invocation ctx.bind override, must each resolve only
  # their own Handles — a foreign owner is a cross-invocation misdelivery on the
  # shared-Sandbox shape (B-22 / B-03).
  def test_release_shared_sandbox_isolates_across_threads
    shared = shared_sandbox
    assert_isolation_across_batches("each :release Thread sharing one Sandbox must resolve only its own " \
                                    "ctx.bind identity (B-22 / B-03)") do |specs|
      run_shared_batch(shared, specs)
    end
  end

  private

  # Drive @iterations worth of THREADS-wide batches through +runner+ (which
  # returns +[tid, kinds, value]+ per Thread) and assert each Thread's decoded
  # result carries only its own tag, then assert shape coverage is complete.
  def assert_isolation_across_batches(message)
    batches = [@iterations / THREADS, 1].max
    batches.times do |batch|
      specs = Array.new(THREADS) { |tid| [tid, *@generator.generate] }
      yield(specs).each do |tid, kinds, value|
        assert_equal expected(kinds, tid), canonicalize(value), failure(batch, "thread #{tid}", message)
      end
    end
    assert_coverage_complete
  end

  # Generate one program, run it under both modes on the shared Sandboxes,
  # and assert :hold decodes to the tag's owners and :release matches :hold.
  def assert_hold_release_parity(hold, release, iter)
    program, kinds = @generator.generate
    want = expected(kinds, 0)
    hold_result = canonicalize(hold.eval(program).value)
    release_result = canonicalize(release.eval(program).value)
    assert_equal want, hold_result, failure(iter, program, ":hold outcome must decode to the minting tag's owners")
    assert_equal hold_result, release_result,
                 failure(iter, program, ":release outcome must equal :hold — releasing changes scheduling only (B-64)")
  end

  # Run one program per Thread on that Thread's own :release Sandbox and
  # return +[tid, kinds, value]+ per Thread; the Sandbox is built inside the
  # Thread so construction runs parallel too.
  def run_batch(specs)
    specs.map do |tid, program, kinds|
      Thread.new do
        sandbox = tagged_sandbox(:release, tid)
        [tid, kinds, sandbox.eval(program).value]
      end
    end.map(&:value)
  end

  # Run one program per Thread against the one +shared+ Sandbox, each Thread
  # filling Vault::Mint with its own tag through the per-invocation ctx.bind
  # override, and return +[tid, kinds, value]+ per Thread.
  def run_shared_batch(shared, specs)
    specs.map do |tid, program, kinds|
      Thread.new do
        value = shared.eval(program) { |ctx| ctx.bind("Vault::Mint", -> { Token.new(tid) }) }.value
        [tid, kinds, value]
      end
    end.map(&:value)
  end

  # A Sandbox in +mode+ whose Vault::Mint statically returns Tokens owned by
  # +tag+, plus the shared resolve / iterate / wrap services.
  def tagged_sandbox(mode, tag)
    Kobako::Sandbox.new(wasm_path: REAL_WASM, gvl: mode).tap do |sandbox|
      sandbox.bind("Vault::Mint", -> { Token.new(tag) })
      bind_vault_services(sandbox)
    end
  end

  # A shared :release Sandbox whose Vault::Mint is a fillable each invocation
  # fills with its own tag through ctx.bind; the resolve / iterate / wrap
  # services stay static, read-only, and shared across Threads.
  def shared_sandbox
    Kobako::Sandbox.new(wasm_path: REAL_WASM, gvl: :release).tap do |sandbox|
      sandbox.bind("Vault::Mint")
      bind_vault_services(sandbox)
    end
  end

  def bind_vault_services(sandbox)
    sandbox.bind("Vault::Owner", lambda(&:owner))
    sandbox.bind("Vault::Each", ->(items, &blk) { items.each(&blk) })
    sandbox.bind("Vault::Wrap", ->(&blk) { blk.call })
  end

  # Reduce a decoded result to owner identities: an owner Integer stays, a
  # restored Token becomes +[:token, owner]+, so two runs compare free of
  # host-object identity.
  def canonicalize(result)
    result.map { |leaf| leaf.is_a?(Integer) ? leaf : [:token, leaf.owner] }
  end

  # The canonical result a program with +kinds+ must yield when Vault::Mint
  # carries +tag+.
  def expected(kinds, tag)
    kinds.map { |kind| kind == :int ? tag : [:token, tag] }
  end

  def assert_coverage_complete
    missing = DispatchProgramGenerator::COVERAGE_KEYS.reject { |key| @generator.coverage[key].positive? }
    assert missing.empty?,
           "fuzz shape-coverage gap (seed=#{@seed}): #{missing.inspect}; counters=#{@generator.coverage.inspect}"
  end

  def failure(iter, program, message)
    "dispatch scheduling fuzz failure at #{iter} (seed=#{@seed})\n  message: #{message}\n  program: #{program}"
  end
end
