# frozen_string_literal: true

require "test_helper"

# E2E (Layer 4) — the J-02 / J-03 / J-04 / J-07 journeys (SPEC.md L161-204,
# L243-254): setup-once / run-many Sandbox reuse, per-submission isolation,
# per-request expression evaluation, and the preload + dispatch-many worker
# pattern through real mruby.
class TestE2ELifecycle < Minitest::Test
  include E2eGuestHelper

  # ── J-02 — Host App developer integrates kobako into an existing service ──
  #
  # SPEC.md L161-173: Setup-once / run-many pattern; same Sandbox resets
  # capability state between #run calls; Service objects bound at setup
  # time remain active across runs without re-registration.

  def test_j02_setup_once_run_many_with_persistent_service_bindings
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.define(:Data).bind(:Fetch, ->(id) { "record:#{id}" })

    a = sandbox.eval('Data::Fetch.call("a")')
    b = sandbox.eval('Data::Fetch.call("b")')

    assert_equal "record:a", a, "J-02: first run sees the binding"
    assert_equal "record:b", b, "J-02: subsequent run still sees the binding (SPEC.md L173)"
  end

  # B-03: one long-lived Sandbox runs #run twice; each invocation executes
  # against the canonical boot state (B-49), so guest runtime state mutated
  # by one invocation cannot survive into the next. This is the isolation
  # invariant the serverless example's Object Pool depends on when it
  # reuses a single preloaded Sandbox across many requests. The Probe
  # returns the global it observed at entry and then sets it: a leak would
  # make the second invocation observe `true` instead of the fresh `nil`.
  def test_j02_reused_sandbox_does_not_leak_guest_globals_between_runs
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.preload(code: "Probe = ->(*_a, **_k) { s = $leak; $leak = true; s }", name: :Probe)

    first = sandbox.run(:Probe)
    second = sandbox.run(:Probe)

    assert_nil first, "J-02 / B-03: first #run on a fresh Sandbox observes an unset guest global"
    assert_nil second,
               "J-02 / B-03: a reused Sandbox must not surface the prior #run's guest global mutation"
  end

  # SPEC.md L169 + B-04: developer reads Sandbox#stdout for guest puts/print
  # output AND the script's return value comes through the outcome envelope.
  # Both channels are independently observable.
  def test_j02_stdout_and_return_value_independently_observable
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)

    result = sandbox.eval(<<~RUBY)
      puts "diagnostic"
      42
    RUBY

    assert_equal 42, result,
                 "J-02 / B-04: return value comes through outcome envelope, not stdout"
    assert_includes sandbox.stdout, "diagnostic",
                    "J-02 / B-04: guest puts is captured in Sandbox#stdout (SPEC.md L169, B-04)"
  end

  # ── J-03 — Teaching platform evaluates student submissions in isolation ──
  #
  # SPEC.md L177-189: Each submission runs in a fresh Sandbox; a failing
  # submission must not affect another submission. No submission can read
  # another submission's guest output.

  def test_j03_fresh_sandbox_per_submission_isolates_failure
    crashing  = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    surviving = Kobako::Sandbox.new(wasm_path: REAL_WASM)

    assert_raises(Kobako::SandboxError) do
      crashing.eval('raise "buggy submission"')
    end

    result = surviving.eval("1 + 1")

    assert_equal 2, result,
                 "J-03: a crashed submission Sandbox must not affect another (SPEC.md L187)"
  end

  # ── J-04 — No-code platform evaluates user-defined expressions per request ──
  #
  # SPEC.md L193-204: Per-tenant Sandbox; each event triggers a Sandbox#eval
  # with a user expression; expression result drives downstream logic.

  def test_j04_user_expression_evaluates_to_value_for_filter_logic
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.define(:Event).bind(:Amount, -> { 150 })

    pass_branch = sandbox.eval("Event::Amount.call > 100")
    fail_branch = sandbox.eval("Event::Amount.call > 1000")

    assert_equal true,  pass_branch, "J-04: user expression evaluates to true (SPEC.md L201)"
    assert_equal false, fail_branch, "J-04: user expression evaluates to false (SPEC.md L201)"
  end

  # J-07 — Host App preloads a worker and dispatches many invocations.
  # SPEC.md L243-254: setup-once / dispatch-many pattern using #preload +
  # #run. Per-invocation isolation (B-03) means no state leaks between
  # successive #run calls on the same Sandbox.
  def test_j07_preload_worker_and_dispatch_many_requests
    sandbox = Kobako::Sandbox.new
    # B-31 (mruby C API limitation): kwargs land as a trailing positional
    # Hash, so entrypoints take a Hash parameter and unpack it themselves.
    # See test/sandbox/test_run.rb:test_b31_passes_keyword_args_as_trailing_positional_hash.
    sandbox.preload(
      code: "class Worker; def self.call(req, opts = {}); req * (opts[:multiplier] || 1); end; end",
      name: :Worker
    )

    assert_equal 2, sandbox.run(:Worker, 2)
    assert_equal 9, sandbox.run(:Worker, 3, multiplier: 3)
    assert_equal 20, sandbox.run(:Worker, 4, multiplier: 5)
  end

  # J-07 follow-up: #run and #eval interleave freely on the same Sandbox;
  # both verbs replay the snippet table from a fresh mrb_state.
  def test_j07_eval_and_run_interleave_with_isolated_state
    sandbox = Kobako::Sandbox.new
    sandbox.preload(code: "Worker = ->(n) { n * n }", name: :Worker)

    assert_equal 16, sandbox.run(:Worker, 4)
    assert_equal 16, sandbox.eval("Worker.call(3) + 7")
    assert_equal 25, sandbox.run(:Worker, 5)
  end
end
