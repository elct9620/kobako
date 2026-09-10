# frozen_string_literal: true

require "test_helper"

# E2E (Layer 4) — the J-01 journey: an LLM agent author runs
# model-generated code with curated capabilities and reads each failure
# back. The Host App developer's journeys live in
# test_journeys_host_app.rb, Sandbox reuse / isolation journeys in
# test_lifecycle.rb.
class TestE2EJourneys < Minitest::Test
  include E2eGuestHelper

  # ── J-01 — LLM agent author runs model-generated code with curated capabilities ──
  #
  # SPEC.md L146-158: The Host App declares Services; generated
  # scripts that exceed declared capabilities receive ServiceError; scripts
  # with Ruby errors raise SandboxError; Wasm-level failures raise TrapError.

  # SPEC.md L152-156: model-generated script calls a curated Service
  # and the Host App receives a deserialized return value.
  # @behavior J-001
  def test_j01_curated_capability_call_returns_deserialized_result
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("KV::Lookup", ->(key) { "value:#{key}" })

    result = sandbox.eval(<<~RUBY).value
      KV::Lookup.call("user_42")
    RUBY

    assert_equal "value:user_42", result,
                 "J-01: model-generated script must receive deserialized Service result (SPEC.md L156)"
  end

  # SPEC.md L157: scripts with Ruby errors raise SandboxError.
  # @behavior J-002
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
  # @behavior J-003
  def test_j01_syntax_error_source_raises_sandbox_error_before_execution
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)

    err = assert_raises(Kobako::SandboxError) do
      sandbox.eval('puts "reached execution"; 1 +')
    end

    assert_equal "sandbox", err.origin,
                 "syntactically invalid source through #eval must raise a sandbox-origin SandboxError"
    assert_empty err.execution.stdout,
                 "source that fails to compile through #eval must not execute the statements preceding the error"
  end

  # A script that never compiled never ran, so it has no backtrace; the
  # message is where the author of generated code reads what to fix.
  # @behavior J-010
  def test_j01_syntax_error_source_names_where_the_parse_stopped
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)

    err = assert_raises(Kobako::SandboxError) do
      sandbox.eval("x = 1\ny = x +")
    end

    assert_match(/\A\(eval\):2:\d+: syntax error/, err.message,
                 "syntactically invalid source through #eval must fail with a message naming the line " \
                 "and column the parse stopped at")
  end

  # SPEC.md "Panic Envelope" L876 — the +backtrace+ field is an array of
  # str carrying the mruby backtrace. The guest must populate it from the
  # mruby Exception object so the Host App can see where the failure
  # originated inside the user script; an empty array hides which line the
  # author needs to fix and forces blind debugging. The host-side decoder
  # already pins the Array-of-String type invariant via the RBS alias
  # +Outcome::panic_fields+,
  # so this E2E only asserts the non-empty contract.
  # @behavior J-004
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
  # @behavior J-005
  def test_j01_capability_error_raises_service_error
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("Log::Sink", ->(_msg) { raise "capability denied" })

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
  # @behavior J-006
  def test_j01_unrescued_service_error_exposes_mruby_backtrace
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("Log::Sink", ->(_msg) { raise "capability denied" })

    err = assert_raises(Kobako::ServiceError) do
      sandbox.eval(<<~RUBY)
        Log::Sink.call("secret")
      RUBY
    end

    refute_empty err.backtrace_lines,
                 "guest must populate Panic.backtrace for service-origin panics too"
  end
end
