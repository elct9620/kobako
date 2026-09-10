# frozen_string_literal: true

require "test_helper"

# Unit-level coverage of Handle invalidity through Transport::Dispatcher:
# a Handle dies with its run and never crosses Sandbox instances. Handle
# resolution itself lives in test_dispatcher_handles.rb.
class TestTransportDispatchInvalidity < Minitest::Test
  include DispatcherHelpers

  # ---------- Cross-run invalidity ----------

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

  # ---------- Cross-Sandbox-instance invalidity ----------

  # Distinct from cross-run invalidity within one Sandbox: here two
  # physically separate Catalog::Handles instances back two separate
  # dispatchers, mirroring two live Sandboxes.
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
    # Same boundary, but the cross-Sandbox handle arrives as a
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
  # uses — the foreign Sandbox A side of the cross-Sandbox boundary.
  def foreign_handle_id(obj)
    Kobako::Catalog::Handles.new.alloc(obj).id
  end

  # A second physically separate [server, table] pair mirroring a second
  # live Sandbox.
  def sandbox_b
    table = Kobako::Catalog::Handles.new
    server = Kobako::Catalog::Services.new
    [server, table]
  end

  # Fixture: object with a single `ping → "pong"` method, the minimum
  # Handle target needed for cross-Sandbox invalidity coverage.
  def pinger
    obj = Object.new
    def obj.ping = "pong"
    obj
  end
end
