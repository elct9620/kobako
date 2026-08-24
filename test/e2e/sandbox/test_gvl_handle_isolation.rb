# frozen_string_literal: true

require "test_helper"

# E2E (Layer 4) — concurrent invocations isolate per invocation under
# gvl: :release, whether Threads use distinct Sandboxes or share one
# (docs/behavior/runtime.md B-22 / B-64). Each invocation runs on its own
# Context, so a Handle minted in one never resolves in another Thread's
# invocation. Two shapes are witnessed:
#
#   * distinct Sandbox per Thread — each Thread's Sandbox mints Tokens
#     tagged with its id; every Thread mints the SAME Handle ids but with
#     distinct owners, so a table shared across Threads would surface as a
#     foreign owner. Handles round-trip both as the result (restored
#     host-side, B-37) and as a dispatch argument (resolved host-side,
#     B-16).
#   * shared Sandbox — all Threads invoke one Sandbox, each supplying its
#     identity through the per-invocation ctx.bind override (B-63) over
#     both #eval and #run; a leaked override or table would surface as a
#     foreign owner.
#
# Correctness is timing-independent — every owner must be the invoking
# Thread's own regardless of interleaving — while the Thread and round
# counts widen the parallel overlap. The sequential guarantee — a Handle
# from invocation N invalid in N+1 — is a separate property covered by the
# cross-invocation invalidity test (B-18).
class TestE2EGvlHandleIsolation < Minitest::Test
  include E2eGuestHelper

  THREADS = 8
  ROUNDS = 8
  HANDLES = 20

  # Per-invocation stateful host object. A Handle minted for it carries no
  # wire representation, so it crosses as a Capability Handle whose +owner+
  # identifies the invocation whose Vault::Mint produced it.
  class Token
    attr_reader :owner

    def initialize(owner) = (@owner = owner)
  end

  # @behavior RT-004
  # A Handle the guest returns as the result is restored to the minting
  # Thread's own host Token, never another Thread's (distinct-Sandbox shape).
  def test_release_restores_each_thread_own_minted_handles
    program = "(0...#{HANDLES}).map { Vault::Mint.call }"

    concurrent_rounds(method(:vault_sandbox)) { |sandbox, _tid| sandbox.eval(program).value }
      .each do |tid, rounds|
        rounds.each do |restored|
          assert_equal [tid] * HANDLES, restored.map(&:owner),
                       "distinct :release Sandboxes minting Handles on distinct Threads must each " \
                       "restore only their own host Tokens — a foreign owner is a cross-invocation " \
                       "misdelivery (B-64 / B-03 / B-37)"
        end
      end
  end

  # @behavior RT-005
  # A Handle the guest passes back as a dispatch argument resolves against
  # the minting Thread's own table, never another Thread's (distinct shape).
  def test_release_resolves_each_thread_own_handle_arguments
    program = "(0...#{HANDLES}).map { Vault::Owner.call(Vault::Mint.call) }"

    concurrent_rounds(method(:vault_sandbox)) { |sandbox, _tid| sandbox.eval(program).value }
      .each do |tid, rounds|
        rounds.each do |owners|
          assert_equal [tid] * HANDLES, owners,
                       "a Handle passed back as a dispatch argument under gvl: :release must resolve " \
                       "against its own invocation's table — a foreign owner is a cross-invocation " \
                       "misdelivery (B-64 / B-03 / B-16)"
        end
      end
  end

  # @behavior RT-002
  # Threads sharing ONE :release Sandbox, each #eval supplying its own
  # identity through the per-invocation ctx.bind override, must each resolve
  # only their own Tokens (shared-Sandbox shape, B-22 / B-63).
  def test_release_shared_sandbox_isolates_per_eval_identity
    shared = shared_sandbox
    program = "(0...#{HANDLES}).map { Vault::Owner.call(Vault::Mint.call) }"

    concurrent_rounds(->(_tid) { shared }) { |sandbox, tid| eval_with_identity(sandbox, program, tid) }
      .each do |tid, rounds|
        rounds.each do |owners|
          assert_equal [tid] * HANDLES, owners,
                       "each Thread sharing one Sandbox must see only its own per-invocation ctx.bind " \
                       "identity through #eval — a foreign owner is a cross-invocation misdelivery (B-22 / B-03)"
        end
      end
  end

  # @behavior RT-003
  # The same isolation over #run: a preloaded entrypoint mints Handles while
  # each Thread's per-run ctx.bind supplies the identity (shared-Sandbox, #run).
  def test_release_shared_sandbox_isolates_per_run_identity
    shared = shared_run_sandbox

    concurrent_rounds(->(_tid) { shared }) { |sandbox, tid| run_with_identity(sandbox, tid) }
      .each do |tid, rounds|
        rounds.each do |owners|
          assert_equal [tid] * HANDLES, owners,
                       "each Thread sharing one Sandbox must see only its own per-invocation ctx.bind " \
                       "identity through #run — a foreign owner is a cross-invocation misdelivery (B-22 / B-03)"
        end
      end
  end

  private

  # Run ROUNDS invocations on THREADS Threads; +provider+ yields the Sandbox
  # a Thread uses — a fresh tagged one per Thread for the distinct shape, or
  # the one shared instance — and the block runs each round, returning one
  # +[tid, rounds]+ pair per Thread.
  def concurrent_rounds(provider, &block)
    (0...THREADS).map do |tid|
      Thread.new do
        sandbox = provider.call(tid)
        [tid, Array.new(ROUNDS) { block.call(sandbox, tid) }]
      end
    end.map(&:value)
  end

  # A :release Sandbox whose Vault::Mint hands back Tokens owned by +tid+ and
  # whose Vault::Owner reads the owner off a Handle argument.
  def vault_sandbox(tid)
    Kobako::Sandbox.new(wasm_path: REAL_WASM, gvl: :release).tap do |sandbox|
      sandbox.bind("Vault::Mint", -> { Token.new(tid) })
      sandbox.bind("Vault::Owner", lambda(&:owner))
    end
  end

  # A :release Sandbox whose Vault::Mint is a fillable each invocation fills
  # with its own tag through ctx.bind; Vault::Owner stays a shared, read-only
  # static binding.
  def shared_sandbox
    Kobako::Sandbox.new(wasm_path: REAL_WASM, gvl: :release).tap do |sandbox|
      sandbox.bind("Vault::Mint")
      sandbox.bind("Vault::Owner", lambda(&:owner))
    end
  end

  # The shared Sandbox plus a preloaded entrypoint that mints and resolves
  # Handles, for the #run path.
  def shared_run_sandbox
    shared_sandbox.tap do |sandbox|
      sandbox.preload(code: "Worker = ->(n) { (0...n).map { Vault::Owner.call(Vault::Mint.call) } }", name: :Worker)
    end
  end

  def eval_with_identity(sandbox, program, tid)
    sandbox.eval(program) { |ctx| ctx.bind("Vault::Mint", -> { Token.new(tid) }) }.value
  end

  def run_with_identity(sandbox, tid)
    sandbox.run(:Worker, HANDLES) { |ctx| ctx.bind("Vault::Mint", -> { Token.new(tid) }) }.value
  end
end
