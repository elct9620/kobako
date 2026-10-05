# frozen_string_literal: true

require "test_helper"

# E2E — the Host App developer's journeys: routing failures through the
# three-class error taxonomy, exposing a block-yielding Service, and
# serving concurrent requests from a warm Sandbox pool. The agent author's
# walk is test_journeys.rb.
class TestE2EHostAppJourneys < Minitest::Test
  include E2eGuestHelper

  # ── Host App developer distinguishes and handles the three error classes ──
  #
  # The three-class taxonomy lets the developer route
  # each failure class through existing error-handling infrastructure.

  # The guest sources a Host App has to route apart, and the class each
  # must arrive as. The script fault is the sandbox layer; the other three
  # are the dispatch failures — a call that reached no Service at all, one
  # whose arguments did not fit, and one the Service answered by raising.
  FAILURES_TO_ROUTE = {
    'raise "script-level fault"' => Kobako::SandboxError,
    "Svc::Call.no_such_method" => Kobako::NoServiceError,
    "Svc::Call.call(1, 2, 3)" => Kobako::ServiceArgumentError,
    'Svc::Call.call("x")' => Kobako::ServiceError
  }.freeze

  # @behavior J-007
  def test_j05_developer_routes_each_failure_by_its_own_class
    FAILURES_TO_ROUTE.each do |source, expected|
      sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
      sandbox.bind("Svc::Call", ->(one) { raise "service exploded: #{one}" })

      err = assert_raises(expected) { sandbox.eval(source) }

      assert_instance_of expected, err,
                         "#{source} through #eval must reach the Host App as #{expected}, so a " \
                         "rescue routes it without reading the message"
    end
  end

  # ── Host App exposes a block-yielding Service ──
  #
  # The whole journey in one walk — an idiomatic host
  # iterator yields each element to the guest-supplied block and the
  # mapped collection flows back; the per-step yield mechanics are pinned
  # in test_yield.rb / test_yield_unwind.rb.

  # @behavior J-008
  def test_j06_block_yielding_service_maps_each_element_through_the_guest_block
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("Service::MyEach", ->(items, &blk) { items.map { |x| blk.call(x) } })

    result = sandbox.eval("Service::MyEach.call([1, 2, 3]) { |x| x * 2 }").value

    assert_equal [2, 4, 6], result,
                 "a block-yielding Service called through #eval must run the guest block once per element " \
                 "and return the mapped collection to the Host App"
  end

  # ── Host App serves concurrent requests from a warm Sandbox pool ──
  #
  # The whole journey in one walk — setup (preload) paid
  # once per pooled Sandbox, then concurrent handlers each run the worker
  # exclusively; checkout/checkin mechanics are pinned in test/pool/.

  # @behavior J-009 PL-025
  def test_j08_concurrent_requests_each_receive_their_own_worker_result
    pool = Kobako::Pool.new(slots: 2) do |sandbox|
      sandbox.preload(code: 'Worker = ->(req) { "done:" + req }', name: :Worker)
    end

    results = Array.new(4) do |i|
      Thread.new { pool.with { |sandbox| sandbox.run(:Worker, "req#{i}").value } }
    end.map(&:value)

    assert_equal %w[done:req0 done:req1 done:req2 done:req3], results.sort,
                 "every concurrent request through Pool#with must receive its own " \
                 "request's worker result"
  end
end
