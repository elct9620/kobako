# frozen_string_literal: true

require "test_helper"

# Coverage for Kobako::Sandbox#run dispatch against the real Guest
# Binary (docs/behavior/invocation.md B-31 + E-27 / E-28, E-04 reuse):
# the success envelope, guest-detected entrypoint failures, and the
# exception envelope. Host pre-flight rejection (E-24 / E-25 / E-29 /
# E-30) needs no guest and lives in test_run_preflight.rb.
class TestSandboxRun < Minitest::Test
  include E2eGuestHelper

  # B-31: a preloaded snippet defines a top-level constant responding to
  # #call; #run dispatches into it and returns the call's value.
  def test_b31_runs_preloaded_entrypoint_with_no_args
    sandbox = Kobako::Sandbox.new
    sandbox.preload(code: "Worker = ->(*_args, **_kw) { 42 }", name: :Worker)

    assert_equal 42, sandbox.run(:Worker),
                 "a preloaded callable entrypoint through #run must return its call value"
  end

  def test_b31_passes_positional_args_to_entrypoint
    sandbox = Kobako::Sandbox.new
    sandbox.preload(code: "Adder = ->(a, b) { a + b }", name: :Adder)

    assert_equal 5, sandbox.run(:Adder, 2, 3),
                 "positional arguments through #run must reach the entrypoint in order"
  end

  # B-31 (mruby C API limitation): kwargs are delivered to the entrypoint as
  # a trailing positional Hash, because `mrb_funcall_argv` forces
  # `ci->nk = 0` on every call (vendor/mruby/src/vm.c:740 — "funcall does not
  # support keyword arguments"). Entrypoints declare a positional Hash
  # parameter (`def call(req, opts = {})` / `->(req, opts) { ... }`) and
  # unpack it themselves; a Ruby-flavour `def call(name:)` signature
  # cannot be reached from the host C side — B-31 accepts the
  # positional-Hash convention instead of routing every #run through an
  # eval shim.
  def test_b31_passes_keyword_args_as_trailing_positional_hash
    sandbox = Kobako::Sandbox.new
    sandbox.preload(code: 'Greeter = ->(opts) { "hello " + opts[:name] }', name: :Greeter)

    assert_equal "hello world", sandbox.run(:Greeter, name: "world"),
                 "kwargs through #run must reach the entrypoint as a trailing positional Hash"
  end

  def test_b31_normalizes_string_target_to_symbol
    sandbox = Kobako::Sandbox.new
    sandbox.preload(code: "Worker = ->(*_args, **_kw) { 7 }", name: :Worker)

    assert_equal 7, sandbox.run("Worker"),
                 "a String target through #run must dispatch like its Symbol form"
  end

  def test_b31_preloaded_snippets_replay_before_dispatch
    sandbox = Kobako::Sandbox.new
    sandbox.preload(code: "BASE = 10", name: :Alpha)
    sandbox.preload(code: "Worker = ->(*_a, **_k) { BASE * 4 }", name: :Beta)

    assert_equal 40, sandbox.run(:Worker),
                 "every preloaded snippet through #run must replay before the entrypoint dispatches"
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
  # when their entrypoint name failed to resolve (docs/behavior/invocation.md B-31).
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

  private

  def run_missing_against_sandbox_with_preloads
    sandbox = Kobako::Sandbox.new
    sandbox.preload(code: "Worker = ->(*_a) { 1 }", name: :Worker)
    sandbox.preload(code: "Helper = Module.new", name: :Helper)
    assert_raises(Kobako::SandboxError) { sandbox.run(:Missing) }
  end
end
