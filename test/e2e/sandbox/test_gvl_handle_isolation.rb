# frozen_string_literal: true

require "test_helper"

# E2E (Layer 4) — concurrent :release invocations on distinct Threads do
# not cross-deliver Capability Handles when gvl: :release lets their guest
# spans run in parallel (docs/behavior/runtime.md B-64). Since the
# one-Thread-per-Sandbox contract (B-22) forbids concurrent invocations on
# one Sandbox, the smallest concurrent unit is the Sandbox, and the risk
# :release newly exposes is shared mutable state in the host driver rather
# than the per-invocation Ruby Context (a distinct object each invocation).
# Every Thread mints the SAME Handle ids on its own Sandbox but tags each
# with a distinct owner, so a table shared across Threads surfaces as a
# foreign owner: the guest rounds each Handle back both as the result
# (restored host-side, B-37) and as a dispatch argument (resolved
# host-side, B-16), and every owner must be the minting Thread's own or an
# "unknown Handle id" ServiceError fires (B-03 / B-64). Correctness is
# timing-independent; the Thread and round counts widen the parallel
# overlap so a latent shared-state regression has room to surface. The
# sequential guarantee — a Handle from invocation N invalid in N+1 — is a
# separate property covered by the cross-invocation invalidity test (B-18).
class TestE2EGvlHandleIsolation < Minitest::Test
  include E2eGuestHelper

  THREADS = 8
  ROUNDS = 8
  HANDLES = 20

  # Per-thread stateful host object. A Handle minted for it carries no
  # wire representation, so it crosses as a Capability Handle whose
  # +owner+ identifies the Thread whose Vault::Mint produced it.
  class Token
    attr_reader :owner

    def initialize(owner) = (@owner = owner)
  end

  # A :release Sandbox whose Vault::Mint hands back Tokens owned by +tid+
  # and whose Vault::Owner reads the owner off a Handle argument, so a
  # Token resolving to a foreign owner exposes a cross-invocation leak.
  def vault_sandbox(tid)
    Kobako::Sandbox.new(wasm_path: REAL_WASM, gvl: :release).tap do |sandbox|
      sandbox.bind("Vault::Mint", -> { Token.new(tid) })
      sandbox.bind("Vault::Owner", lambda(&:owner))
    end
  end

  # Run +program+ on each Thread's own :release Sandbox ROUNDS times and
  # return one +[tid, rounds]+ pair per Thread, where +rounds+ is that
  # Thread's per-round +#eval+ values. The Sandbox is built inside the
  # Thread so construction runs parallel too.
  def each_thread_rounds(program)
    (0...THREADS).map do |tid|
      Thread.new do
        sandbox = vault_sandbox(tid)
        [tid, ROUNDS.times.map { sandbox.eval(program).value }]
      end
    end.map(&:value)
  end

  # A Handle the guest received and returns as the result is restored to
  # the minting Thread's own host Token, never another Thread's.
  def test_release_restores_each_thread_own_minted_handles
    program = "(0...#{HANDLES}).map { Vault::Mint.call }"

    each_thread_rounds(program).each do |tid, rounds|
      rounds.each do |restored|
        assert_equal [tid] * HANDLES, restored.map(&:owner),
                     "distinct :release Sandboxes minting Handles on distinct Threads must each " \
                     "restore only their own host Tokens — a foreign owner is a cross-invocation " \
                     "misdelivery (B-64 / B-03 / B-37)"
      end
    end
  end

  # A Handle the guest passes back as a dispatch argument resolves against
  # the minting Thread's own table, never another Thread's.
  def test_release_resolves_each_thread_own_handle_arguments
    program = "(0...#{HANDLES}).map { Vault::Owner.call(Vault::Mint.call) }"

    each_thread_rounds(program).each do |tid, rounds|
      rounds.each do |owners|
        assert_equal [tid] * HANDLES, owners,
                     "a Handle passed back as a dispatch argument under gvl: :release must resolve " \
                     "against its own invocation's table — a foreign owner is a cross-invocation " \
                     "misdelivery (B-64 / B-03 / B-16)"
      end
    end
  end
end
