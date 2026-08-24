# frozen_string_literal: true

require "test_helper"

# Unit-level coverage of Handle invalidity through Transport::Dispatcher:
# a Handle dies with its run (B-18) and never crosses Sandbox instances
# (B-19). Handle resolution itself lives in test_dispatcher_handles.rb.
class TestTransportDispatchInvalidity < Minitest::Test
  include DispatcherHelpers

  # ---------- Cross-run invalidity (SPEC B-18) ----------

  # @behavior T-038
  def test_a_prior_runs_handle_is_undefined_against_the_next_runs_table
    obj = Object.new
    def obj.tag = "t"
    handle_id = alloc_id(obj) # issued against this run's table (@handler)
    next_run = Kobako::Catalog::Handles.new # the next invocation mints its own

    resp = dispatch_handle_target(handle_id, "tag", handler: next_run)

    assert_predicate resp, :error?
    assert_equal "undefined", resp.payload.type
  end

  # ---------- Cross-Sandbox-instance invalidity (SPEC B-19) ----------

  # SPEC B-19: Handle IDs are Sandbox-private. A Handle ID issued by
  # Sandbox A's Catalog::Handles has no meaning in Sandbox B's Catalog::Handles;
  # presenting it there resolves to "ID not found" and surfaces as a
  # the fault arm with type="undefined". Distinct from B-18 (cross-#run
  # within one Sandbox): here two physically separate Catalog::Handles
  # instances back two separate dispatchers, mirroring two live Sandboxes.
  # @behavior T-039
  def test_handle_from_sandbox_a_is_undefined_in_sandbox_b_as_target
    table_a = Kobako::Catalog::Handles.new
    handle_id_in_a = table_a.alloc(pinger).id
    server_b, table_b = sandbox_b

    # The integer id has meaning in A but must NOT cross over to B —
    # B's Catalog::Handles does not contain that id.
    assert_equal "pong", table_a.fetch(handle_id_in_a).ping
    resp = dispatch_handle_target(handle_id_in_a, "ping", server: server_b, handler: table_b)

    assert_predicate resp, :error?
    assert_equal "undefined", resp.payload.type
    assert_equal 0, table_b.size
  end

  # @behavior T-040
  def test_handle_from_sandbox_a_is_undefined_in_sandbox_b_as_arg
    # Same B-19 boundary, but the cross-Sandbox handle arrives as a
    # positional arg rather than the target. The Server path resolves;
    # arg resolution fails when the id misses B's Catalog::Handles.
    handle_id_in_a = foreign_handle_id(Object.new)
    server_b, table_b = sandbox_b
    server_b.bind("Echo::Wrap", ->(g) { "wrapped:#{g}" })

    call = build_call("Echo::Wrap", "call", [Kobako::Handle.restore(handle_id_in_a)], {})
    answer = reify(dispatch(call, server: server_b, handler: table_b))

    assert_predicate answer, :error?
    assert_equal "undefined", answer.payload.type
  end

  private

  # Allocate +obj+ in a Catalog::Handles that no dispatcher under test
  # uses — the foreign Sandbox A side of the B-19 boundary.
  def foreign_handle_id(obj)
    Kobako::Catalog::Handles.new.alloc(obj).id
  end

  # A second physically separate [server, table] pair mirroring a second
  # live Sandbox (B-19).
  def sandbox_b
    table = Kobako::Catalog::Handles.new
    server = Kobako::Catalog::Services.new
    [server, table]
  end

  # Fixture: object with a single `ping → "pong"` method, the minimum
  # Handle target needed for cross-Sandbox B-19 invalidity coverage.
  def pinger
    obj = Object.new
    def obj.ping = "pong"
    obj
  end
end
