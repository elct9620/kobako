# frozen_string_literal: true

require "test_helper"

# E2E (Layer 4) — the J-01, J-05, J-06, and J-08 journeys: an LLM agent
# author runs model-generated code with curated capabilities, the Host App
# developer routes failures through the three-class error taxonomy, exposes
# a block-yielding Service, and serves concurrent requests from a warm
# Sandbox pool. Sandbox reuse / isolation journeys live in
# test_lifecycle.rb.
class TestE2EJourneys < Minitest::Test
  include E2eGuestHelper

  # ── J-01 — LLM agent author runs model-generated code with curated capabilities ──
  #
  # SPEC.md L146-158: The Host App declares Service namespaces; generated
  # scripts that exceed declared capabilities receive ServiceError; scripts
  # with Ruby errors raise SandboxError; Wasm-level failures raise TrapError.

  # SPEC.md L152-156: model-generated script calls a curated Member
  # and the Host App receives a deserialized return value.
  def test_j01_curated_capability_call_returns_deserialized_result
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.define(:KV).bind(:Lookup, ->(key) { "value:#{key}" })

    result = sandbox.eval(<<~RUBY)
      KV::Lookup.call("user_42")
    RUBY

    assert_equal "value:user_42", result,
                 "J-01: model-generated script must receive deserialized Service result (SPEC.md L156)"
  end

  # SPEC.md L157: scripts with Ruby errors raise SandboxError.
  def test_j01_script_ruby_error_raises_sandbox_error
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)

    err = assert_raises(Kobako::SandboxError) do
      sandbox.eval(<<~RUBY)
        raise "model produced bad code"
      RUBY
    end

    assert_equal "sandbox", err.origin, "an unrescued script error through #eval must be sandbox-origin"
    refute_kind_of Kobako::ServiceError, err, "a script fault through #eval must not surface as ServiceError"
    refute_kind_of Kobako::TrapError, err, "a script fault through #eval must not surface as TrapError"
  end

  # docs/behavior/errors.md E-05: source that fails to compile is rejected
  # before any execution begins, so a syntactically invalid script — the
  # common shape of model-generated code — raises SandboxError and never
  # runs the statements preceding the error.
  def test_j01_syntax_error_source_raises_sandbox_error_before_execution
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)

    err = assert_raises(Kobako::SandboxError) do
      sandbox.eval('puts "reached execution"; 1 +')
    end

    assert_equal "sandbox", err.origin,
                 "syntactically invalid source through #eval must raise a sandbox-origin SandboxError"
    assert_empty sandbox.stdout,
                 "source that fails to compile through #eval must not execute the statements preceding the error"
  end

  # SPEC.md "Panic Envelope" L876 — the +backtrace+ field is an array of
  # str carrying the mruby backtrace. The guest must populate it from the
  # mruby Exception object so the Host App can see where the failure
  # originated inside the user script; an empty array hides which line the
  # author needs to fix and forces blind debugging. The host-side decoder
  # already pins the Array-of-String type invariant via +Outcome::Panic+,
  # so this E2E only asserts the non-empty contract.
  def test_j01_script_ruby_error_exposes_mruby_backtrace
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    err = assert_raises(Kobako::SandboxError) do
      sandbox.eval(<<~RUBY)
        def boom
          raise "model produced bad code"
        end
        boom
      RUBY
    end
    refute_empty err.backtrace_lines, "SPEC L876: guest must populate Panic.backtrace"
  end

  # SPEC.md L157: Service capability call that errors → ServiceError.
  def test_j01_capability_error_raises_service_error
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.define(:Log).bind(:Sink, ->(_msg) { raise "capability denied" })

    err = assert_raises(Kobako::ServiceError) do
      sandbox.eval(<<~RUBY)
        Log::Sink.call("secret")
      RUBY
    end

    assert_equal "service", err.origin, "an unrescued capability failure through #eval must be service-origin"
    refute_kind_of Kobako::SandboxError, err, "a capability fault through #eval must not surface as SandboxError"
  end

  # SPEC.md L876 again — an unrescued Service call equally flows through
  # the Panic envelope, so its backtrace must also reach the Host App.
  # Otherwise an LLM-generated script that calls a misbehaving capability
  # would surface as ServiceError with no debugging context at all.
  def test_j01_unrescued_service_error_exposes_mruby_backtrace
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.define(:Log).bind(:Sink, ->(_msg) { raise "capability denied" })

    err = assert_raises(Kobako::ServiceError) do
      sandbox.eval(<<~RUBY)
        Log::Sink.call("secret")
      RUBY
    end

    refute_empty err.backtrace_lines,
                 "guest must populate Panic.backtrace for service-origin panics too"
  end

  # ── J-05 — Host App developer distinguishes and handles the three error classes ──
  #
  # SPEC.md L208-220: The three-class taxonomy lets the developer route
  # each failure class through existing error-handling infrastructure.

  def test_j05_developer_distinguishes_three_error_classes
    sandbox_a = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox_b = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox_b.define(:Svc).bind(:Call, ->(_) { raise "service exploded" })

    # SPEC.md L215: SandboxError — script-level fault.
    assert_raises(Kobako::SandboxError) do
      sandbox_a.eval('raise "script-level fault"')
    end

    # SPEC.md L216: ServiceError — capability-level fault.
    err = assert_raises(Kobako::ServiceError) do
      sandbox_b.eval('Svc::Call.call("x")')
    end
    assert_equal "service", err.origin, "J-05: a capability fault through #eval must carry service origin"
  end

  # ── J-06 — Host App exposes a block-yielding Service ──
  #
  # SPEC.md L241-255: the whole journey in one walk — an idiomatic host
  # iterator yields each element to the guest-supplied block and the
  # mapped collection flows back; the per-step yield mechanics are pinned
  # in test_yield.rb / test_yield_unwind.rb.

  def test_j06_block_yielding_service_maps_each_element_through_the_guest_block
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.define(:Service).bind(:MyEach, ->(items, &blk) { items.map { |x| blk.call(x) } })

    result = sandbox.eval("Service::MyEach.call([1, 2, 3]) { |x| x * 2 }")

    assert_equal [2, 4, 6], result,
                 "J-06: a block-yielding Service must run the guest block once per element " \
                 "and return the mapped collection to the Host App"
  end

  # ── J-08 — Host App serves concurrent requests from a warm Sandbox pool ──
  #
  # SPEC.md L271-283: the whole journey in one walk — setup (preload) paid
  # once per pooled Sandbox, then concurrent handlers each run the worker
  # exclusively; checkout/checkin mechanics are pinned in test/pool/.

  def test_j08_concurrent_requests_each_receive_their_own_worker_result
    pool = Kobako::Pool.new(slots: 2) do |sandbox|
      sandbox.preload(code: 'Worker = ->(req) { "done:" + req }', name: :Worker)
    end

    results = Array.new(4) do |i|
      Thread.new { pool.with { |sandbox| sandbox.run(:Worker, "req#{i}") } }
    end.map(&:value)

    assert_equal %w[done:req0 done:req1 done:req2 done:req3], results.sort,
                 "J-08: every concurrent request through Pool#with must receive its own " \
                 "request's worker result"
  end
end
