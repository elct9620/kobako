# frozen_string_literal: true

require "test_helper"
require "support/rendezvous"

# E2E — a Service bound once on a Sandbox shared across Threads is not
# serialized by kobako. Each call waits, bounded, for the other to arrive,
# so the two can only meet if both are inside the Service at once.
class TestE2EGvlSharedService < Minitest::Test
  include E2eGuestHelper

  # @behavior RT-066
  def test_a_shared_service_is_called_by_several_invocations_at_once
    rendezvous = Rendezvous.new(2)
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM, gvl: :release)
    sandbox.bind("Gate::Meet", -> { rendezvous.meet })

    met = Array.new(2) { Thread.new { sandbox.eval("Gate::Meet.call").value } }.map(&:value)

    assert_equal [true, true], met,
                 "two invocations on one shared Sandbox must be inside its Service at once, not serialized"
  end
end
