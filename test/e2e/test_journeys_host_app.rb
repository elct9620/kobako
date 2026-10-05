# frozen_string_literal: true

require "test_helper"
require "support/in_memory_file_system"
require "support/extension_fixtures"

# E2E — the Host App's journeys: routing failures through the three-class
# error taxonomy, exposing a block-yielding Service, serving concurrent
# requests from a warm Sandbox pool, installing a native idiom, and
# evaluating submissions under a deadline. The agent author's walk is
# test_journeys.rb.
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

  # ── Host App installs an Extension so guest code uses a native idiom ──
  #
  # One script needs both halves of the idiom: the path is built in the
  # guest and the read crosses to the backend. Each half alone is pinned in
  # test_install.rb.

  # @behavior J-011
  def test_j09_installed_idiom_answers_a_script_needing_guest_and_host
    store = InMemoryFileSystem.new
    store.write("notes/today.txt", "ship it")
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.install(Kobako::Extension.new(name: :File, source: ExtensionFixtures::FILE_SOURCE,
                                          backend: Kobako::Extension::Backend.new(path: "File", object: store)))

    result = sandbox.eval('File.read(File.join("notes", "today.txt"))').value

    assert_equal "ship it", result,
                 "a script building a path in the guest and reading it through an installed idiom " \
                 "must reach the Host App with what the backend held at that path"
  end

  # ── Teaching platform evaluates submissions under a deadline ──
  #
  # The operator evaluates every submission in turn; one never ends. What
  # the walk reaches is that the others still report, not how the deadline
  # fires — that is pinned in test_caps.rb.

  SUBMISSIONS = ["1 + 1", "loop { }", '"done"'].freeze

  # @behavior J-012
  def test_j03_runaway_submission_costs_the_others_nothing
    results = SUBMISSIONS.map do |source|
      Kobako::Sandbox.new(wasm_path: REAL_WASM, timeout: 0.2).eval(source).value
    rescue Kobako::TimeoutError
      :timed_out
    end

    assert_equal [2, :timed_out, "done"], results,
                 "submissions evaluated in turn under a deadline must each reach the operator with " \
                 "their own result, the one that never ends included only as cut off"
  end
end
