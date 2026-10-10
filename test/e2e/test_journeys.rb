# frozen_string_literal: true

require "test_helper"

# E2E — the journey in which an LLM agent author runs
# model-generated code with curated capabilities and reads each failure
# back. The Host App developer's journeys live in
# test_journeys_host_app.rb, Sandbox reuse / isolation journeys in
# test_lifecycle.rb.
class TestE2EJourneys < Minitest::Test
  include E2eGuestHelper

  # ── LLM agent author runs model-generated code with curated capabilities ──
  #
  # The Host App declares Services; generated
  # scripts that exceed declared capabilities receive ServiceError; scripts
  # with Ruby errors raise SandboxError; Wasm-level failures raise TrapError.

  # A model-generated script calls a curated Service
  # and the Host App receives a deserialized return value.
  # @behavior J-001
  def test_j01_curated_capability_call_returns_deserialized_result
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("KV::Lookup", ->(key) { "value:#{key}" })

    result = sandbox.eval(<<~RUBY).value
      KV::Lookup.call("user_42")
    RUBY

    assert_equal "value:user_42", result,
                 "a curated Service call through #eval must return the deserialized Service result"
  end

  # Scripts with Ruby errors raise SandboxError.
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

  # Source that fails to compile is rejected before any execution begins, so a syntactically invalid script — the
  # common shape of model-generated code — raises SandboxError and never
  # runs the statements preceding the error.
  # @behavior J-003
  def test_j01_syntax_error_source_raises_sandbox_error_before_execution
    assert_equal "sandbox", syntax_error_failure.origin,
                 "syntactically invalid source through #eval must raise a sandbox-origin SandboxError"
  end

  # @behavior J-017
  def test_j01_syntax_error_source_writes_nothing
    assert_empty syntax_error_failure.execution.stdout,
                 "source that fails to compile through #eval must not execute the statements preceding the error"
  end

  # A script that never compiled never ran, so it has no backtrace; the
  # message is where the author of generated code reads what to fix.
  # @behavior J-010 S-129
  def test_j01_syntax_error_source_names_where_the_parse_stopped
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)

    err = assert_raises(Kobako::SandboxError) do
      sandbox.eval("x = 1\ny = x +")
    end

    assert_match(/\A\(eval\):2:\d+: syntax error/, err.message,
                 "syntactically invalid source through #eval must fail with a message naming the line " \
                 "and column the parse stopped at")
  end

  # The guest populates the panic backtrace from the mruby Exception so the
  # Host App sees which line of the user script failed; the host-side
  # decoder already pins its Array-of-String type, so this asserts only that
  # it is non-empty.
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
    refute_empty err.backtrace_lines, "an unrescued script error through #eval must carry its mruby backtrace"
  end

  # A Service capability call that errors raises ServiceError.
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

  # An unrescued Service error equally carries its backtrace, so a
  # misbehaving capability never surfaces with no debugging context.
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

  private

  # Source that writes output and then fails to parse.
  def syntax_error_failure
    assert_raises(Kobako::SandboxError) do
      Kobako::Sandbox.new(wasm_path: REAL_WASM).eval('puts "reached execution"; 1 +')
    end
  end
end
