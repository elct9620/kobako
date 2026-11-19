# frozen_string_literal: true

require "test_helper"
require "support/rendezvous"

# E2E — Threads sharing one Sandbox, each invocation's Context replacing a
# mount or a network entry: what one Context gives stays with its own
# invocation while the others run at once. Each round's invocations meet
# inside their Context blocks after replacing, so every replacement is made
# before any of them runs.
class TestE2EContextReplacementsAtOnce < Minitest::Test
  include E2eGuestHelper

  TENANTS = 8
  ROUNDS = 10

  MOUNT_SCRIPT = 'File.write("/data/mine.txt", "x"); [File.read("/data/who.txt"), Dir.children("/data").sort]'
  LOOKUP_SCRIPT = 'begin; Addrinfo.getaddrinfo("t%<tenant>d.example", 443); rescue StandardError; end; nil'

  # A network entry that refuses every lookup, recording which tenant's
  # entry judged which name.
  RecordingEntry = Struct.new(:tenant, :judged) do
    def resolve?(name)
      judged << [tenant, name]
      false
    end
  end

  # @behavior MT-063
  def test_invocations_at_once_each_reach_only_the_mount_their_own_context_gave
    shared = Kobako::Filesystem::Tmpfs.new("who.txt" => "shared")
    sandbox = release_sandbox.mount("/data", shared, access: :read_write)

    assert_equal expected_views, at_once { |tenant, round| read_own_mount(sandbox, tenant, round) },
                 "invocations at once on one shared Sandbox, each with its own ctx.mount, must each read and " \
                 "write only their own Tmpfs"
    assert_equal ["shared", %w[who.txt]], sandbox.eval('[File.read("/data/who.txt"), Dir.children("/data")]').value,
                 "ctx.mount writes at once must leave the Sandbox's own mount as it was"
  end

  # @behavior NW-234
  def test_invocations_at_once_each_reach_only_the_entry_their_own_context_gave
    judged = Queue.new
    sandbox = release_sandbox.network(:api, RecordingEntry.new(:shared, judged))
    at_once { |tenant, round| look_up_through_own_entry(sandbox, tenant, round, judged) }

    assert_equal expected_lookups, drain(judged),
                 "lookups at once on one shared Sandbox, each with its own ctx.network, must each be judged by " \
                 "their own invocation's entry alone"
    sandbox.eval(format(LOOKUP_SCRIPT, tenant: 0))
    assert_equal [[:shared, "t0.example"]], drain(judged),
                 "ctx.network replacements at once must leave the Sandbox's own entry judging the next lookup"
  end

  private

  def release_sandbox
    Kobako::Sandbox.new(wasm_path: REAL_WASM, gvl: :release)
  end

  # Run ROUNDS rounds on each of TENANTS Threads at once, each round handed
  # the meeting point its invocations share, answering each Thread's
  # distinct results paired with its tenant.
  def at_once(&block)
    meetings = Array.new(ROUNDS) { Rendezvous.new(TENANTS) }
    Array.new(TENANTS) do |tenant|
      Thread.new { meetings.map { |round| block.call(tenant, round) }.uniq.map { |result| [tenant, result] } }
    end.flat_map(&:value)
  end

  def read_own_mount(sandbox, tenant, round)
    own = Kobako::Filesystem::Tmpfs.new("who.txt" => "t#{tenant}")
    sandbox.eval(MOUNT_SCRIPT) { |ctx| ctx.mount("/data", own, access: :read_write) && round.meet }.value
  end

  def look_up_through_own_entry(sandbox, tenant, round, judged)
    own = RecordingEntry.new(tenant, judged)
    sandbox.eval(format(LOOKUP_SCRIPT, tenant: tenant)) { |ctx| ctx.network(:api, own) && round.meet }
  end

  def expected_views
    Array.new(TENANTS) { |tenant| [tenant, ["t#{tenant}", %w[mine.txt who.txt]]] }
  end

  def expected_lookups
    Array.new(TENANTS) { |tenant| [tenant, "t#{tenant}.example"] }
  end

  def drain(queue)
    Array.new(queue.size) { queue.pop }.uniq.sort_by(&:inspect)
  end
end
