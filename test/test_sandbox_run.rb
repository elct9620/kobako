# frozen_string_literal: true

require "test_helper"

# Coverage for Kobako::Sandbox#run — the entrypoint dispatch verb
# (docs/behavior.md B-31 + E-24..E-31).
#
# Host pre-flight cases (E-24 / E-25 / E-29 / E-30) raise standard Ruby
# exceptions synchronously and do not need the real guest binary; the
# fixture-driven tests at the top of the class exercise those paths.
# Guest-detected cases (E-27 / E-28) and the success/exception envelopes
# (B-31 result / E-04 reuse) drive the real data/kobako.wasm and are
# guarded by `defined?(Kobako::Wasm::Instance)`.
class TestSandboxRun < Minitest::Test
  FIXTURE_PATH = File.expand_path("fixtures/minimal.wasm", __dir__)

  def setup
    skip "native ext not compiled (run `bundle exec rake compile`)" unless defined?(Kobako::Wasm::Instance)
    skip "minimal.wasm fixture missing" unless File.exist?(FIXTURE_PATH)
    @fixture_sandbox = Kobako::Sandbox.new(wasm_path: FIXTURE_PATH)
  end

  # --- Host pre-flight (E-24 / E-25 / E-29 / E-30) ---

  # E-24
  def test_e24_target_must_be_symbol_or_string
    err = assert_raises(TypeError) { @fixture_sandbox.run(42) }
    assert_match(/Symbol or String/, err.message)
  end

  # E-25
  def test_e25_target_must_match_constant_pattern
    err = assert_raises(ArgumentError) { @fixture_sandbox.run(:lowercase) }
    assert_match(/must match/, err.message)
  end

  # E-25: `::`-segmented names fail the pattern check at host pre-flight.
  def test_e25_target_rejects_double_colon_segmented_name
    err = assert_raises(ArgumentError) { @fixture_sandbox.run("Outer::Inner") }
    assert_match(/must match/, err.message)
  end

  # E-29
  def test_e29_args_must_not_contain_handle
    handle = Kobako::Handle.new(1)
    err = assert_raises(ArgumentError) { @fixture_sandbox.run(:Worker, handle) }
    assert_match(/Handle/, err.message)
  end

  # E-30
  def test_e30_kwargs_keys_must_be_symbols
    err = assert_raises(ArgumentError) { @fixture_sandbox.run(:Worker, **{ "bad" => 1 }) }
    assert_match(/kwargs keys must be Symbols/, err.message)
  end

  # --- Guest-driven (real data/kobako.wasm) ---

  # B-31: a preloaded snippet defines a top-level constant responding to
  # #call; #run dispatches into it and returns the call's value.
  def test_b31_runs_preloaded_entrypoint_with_no_args
    sandbox = Kobako::Sandbox.new
    sandbox.preload(code: "Worker = ->(*_args, **_kw) { 42 }", name: :Worker)

    assert_equal 42, sandbox.run(:Worker)
  end

  def test_b31_passes_positional_args_to_entrypoint
    sandbox = Kobako::Sandbox.new
    sandbox.preload(code: "Adder = ->(a, b) { a + b }", name: :Adder)

    assert_equal 5, sandbox.run(:Adder, 2, 3)
  end

  # B-31 (mruby C API limitation): kwargs are delivered to the entrypoint as
  # a trailing positional Hash, because `mrb_funcall_argv` forces
  # `ci->nk = 0` on every call (vendor/mruby/src/vm.c:740 — "funcall does not
  # support keyword arguments"). Entrypoints declare a positional Hash
  # parameter (`def call(req, opts = {})` / `->(req, opts) { ... }`) and
  # unpack it themselves; a Ruby-flavour `def call(name:)` signature
  # cannot be reached from the host C side without re-introducing the
  # wrapper-eval workaround.
  def test_b31_passes_keyword_args_as_trailing_positional_hash
    sandbox = Kobako::Sandbox.new
    sandbox.preload(code: 'Greeter = ->(opts) { "hello " + opts[:name] }', name: :Greeter)

    assert_equal "hello world", sandbox.run(:Greeter, name: "world")
  end

  def test_b31_normalizes_string_target_to_symbol
    sandbox = Kobako::Sandbox.new
    sandbox.preload(code: "Worker = ->(*_args, **_kw) { 7 }", name: :Worker)

    assert_equal 7, sandbox.run("Worker")
  end

  def test_b31_preloaded_snippets_replay_before_dispatch
    sandbox = Kobako::Sandbox.new
    sandbox.preload(code: "BASE = 10", name: :Alpha)
    sandbox.preload(code: "Worker = ->(*_a, **_k) { BASE * 4 }", name: :Beta)

    assert_equal 40, sandbox.run(:Worker)
  end

  # E-27: target Symbol does not resolve to a defined top-level constant.
  # Surfaces as Kobako::SandboxError via the guest's Panic envelope path.
  def test_e27_undefined_entrypoint_raises_sandbox_error
    sandbox = Kobako::Sandbox.new
    err = assert_raises(Kobako::SandboxError) { sandbox.run(:Missing) }
    assert_match(/undefined entrypoint: Missing/, err.message)
  end

  # E-27 details: the panic envelope carries the snippet-contributed
  # top-level constants so callers can see what was actually available
  # when their entrypoint name failed to resolve (docs/behavior.md B-31).
  def test_e27_details_includes_snippet_contributed_constants
    err = run_missing_against_sandbox_with_preloads
    available = err.details.fetch("available")
    assert_includes available, :Worker
    assert_includes available, :Helper
  end

  # E-27 details (baseline filtering): kobako-installed runtime classes and
  # mruby builtins are subtracted from `available`, so callers only see
  # constants introduced by the preloaded snippets themselves.
  def test_e27_details_filters_baseline_constants
    err = run_missing_against_sandbox_with_preloads
    available = err.details.fetch("available")
    refute_includes available, :Object
    refute_includes available, :Kobako
  end

  private

  def run_missing_against_sandbox_with_preloads
    sandbox = Kobako::Sandbox.new
    sandbox.preload(code: "Worker = ->(*_a) { 1 }", name: :Worker)
    sandbox.preload(code: "Helper = Module.new", name: :Helper)
    assert_raises(Kobako::SandboxError) { sandbox.run(:Missing) }
  end

  # E-28: entrypoint constant is defined but does not respond to #call.
  def test_e28_entrypoint_without_call_raises_sandbox_error
    sandbox = Kobako::Sandbox.new
    sandbox.preload(code: "Worker = 42", name: :Worker)

    err = assert_raises(Kobako::SandboxError) { sandbox.run(:Worker) }
    assert_match(/does not respond to :call/, err.message)
  end

  # E-04 reuse: entrypoint raises an uncaught exception.
  def test_entrypoint_runtime_exception_surfaces_as_sandbox_error
    sandbox = Kobako::Sandbox.new
    sandbox.preload(code: 'Worker = ->(*_) { raise "boom from worker" }', name: :Worker)

    err = assert_raises(Kobako::SandboxError) { sandbox.run(:Worker) }
    assert_match(/boom from worker/, err.message)
  end
end
