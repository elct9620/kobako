# frozen_string_literal: true

require "test_helper"

# Coverage for Kobako::Sandbox#run dispatch against the real Guest
# Binary: the success envelope, guest-detected entrypoint failures, and
# the exception envelope. Host pre-flight rejection needs no guest and
# lives in test_run_preflight.rb.
class TestSandboxRun < Minitest::Test
  include E2eGuestHelper

  # A preloaded snippet defines a top-level constant responding to #call;
  # #run dispatches into it and returns the call's value.
  # @behavior S-045 S-140
  def test_runs_preloaded_entrypoint_with_no_args
    sandbox = Kobako::Sandbox.new
    sandbox.preload(code: "Worker = ->(*_args, **_kw) { 42 }", name: :Worker)

    assert_equal 42, sandbox.run(:Worker).value,
                 "a preloaded callable entrypoint through #run must return its call value"
  end

  # @behavior S-046 S-139
  def test_passes_positional_args_to_entrypoint
    sandbox = Kobako::Sandbox.new
    sandbox.preload(code: "Adder = ->(a, b) { a + b }", name: :Adder)

    assert_equal 5, sandbox.run(:Adder, 2, 3).value,
                 "positional arguments through #run must reach the entrypoint in order"
  end

  # mruby C API limitation: `mrb_funcall_argv` supports no keyword
  # arguments, so kwargs reach the entrypoint as a trailing positional Hash
  # the entrypoint unpacks itself — accepted instead of routing every #run
  # through an eval shim.
  # @behavior S-047
  def test_passes_keyword_args_as_trailing_positional_hash
    sandbox = Kobako::Sandbox.new
    sandbox.preload(code: 'Greeter = ->(opts) { "hello " + opts[:name] }', name: :Greeter)

    assert_equal "hello world", sandbox.run(:Greeter, name: "world").value,
                 "kwargs through #run must reach the entrypoint as a trailing positional Hash"
  end

  # @behavior S-048
  def test_normalizes_string_target_to_symbol
    sandbox = Kobako::Sandbox.new
    sandbox.preload(code: "Worker = ->(*_args, **_kw) { 7 }", name: :Worker)

    assert_equal 7, sandbox.run("Worker").value,
                 "a String target through #run must dispatch like its Symbol form"
  end

  # @behavior S-049
  def test_preloaded_snippets_replay_before_dispatch
    sandbox = Kobako::Sandbox.new
    sandbox.preload(code: "BASE = 10", name: :Alpha)
    sandbox.preload(code: "Worker = ->(*_a, **_k) { BASE * 4 }", name: :Beta)

    assert_equal 40, sandbox.run(:Worker).value,
                 "every preloaded snippet through #run must replay before the entrypoint dispatches"
  end

  # A target Symbol that resolves to no top-level constant surfaces as the
  # UndefinedEntrypointError subclass, which a caller rescuing plain
  # SandboxError still catches.
  # @behavior S-151
  def test_undefined_entrypoint_raises_the_named_subclass
    sandbox = Kobako::Sandbox.new
    err = assert_raises(Kobako::UndefinedEntrypointError) { sandbox.run(:Missing) }
    assert_match(/undefined entrypoint: Missing/, err.message)
    assert_equal :Missing, err.name,
                 "an unresolved entrypoint through #run must name the target the caller asked for"
  end

  # The error carries the snippet-contributed top-level constants so
  # callers can correct the name from the error itself, without reading the
  # guest source.
  # @behavior S-152
  def test_available_includes_snippet_contributed_constants
    err = run_missing_against_sandbox_with_preloads
    assert_includes err.available, :Worker
    assert_includes err.available, :Helper
  end

  # Baseline filtering: kobako-installed runtime classes and mruby builtins
  # are subtracted, so callers only see constants introduced by the
  # preloaded snippets themselves.
  # @behavior S-152
  def test_available_filters_baseline_constants
    err = run_missing_against_sandbox_with_preloads
    refute_includes err.available, :Object
    refute_includes err.available, :Kobako
  end

  # Bound-Service filtering: a Service the preamble materialises is
  # not a name the caller could have dispatched, so the namespace each bind
  # path roots at is subtracted too — at every path depth, and whether or
  # not a snippet touched it. The witness a registry needs: the two
  # baseline cases above bind nothing, so neither can tell the boot-state
  # subtraction apart from one that also covers the preamble.
  # @behavior S-152
  def test_available_excludes_bound_service_namespaces
    sandbox = Kobako::Sandbox.new
    sandbox.bind("Ledger", -> { 1 })
    sandbox.bind("Shop::Cart", -> { 2 })
    sandbox.bind("Deep::Nested::Svc", -> { 3 })
    sandbox.preload(code: "Worker = ->(*_a) { 1 }", name: :Worker)

    err = assert_raises(Kobako::UndefinedEntrypointError) { sandbox.run(:Missing) }

    assert_equal [:Worker], err.available,
                 "a Sandbox carrying bound Services through #run must offer only the " \
                 "snippet-contributed constants as the unresolved entrypoint's correction"
  end

  # @behavior S-081
  # A constant that exists but answers no call is a different mistake
  # from one that is not there, so the message says which — a Host App
  # correcting the entrypoint needs to know it named the right constant.
  def test_entrypoint_without_call_raises_sandbox_error
    sandbox = Kobako::Sandbox.new
    sandbox.preload(code: "Worker = 42", name: :Worker)

    err = assert_raises(Kobako::SandboxError) { sandbox.run(:Worker) }
    assert_match(/does not respond to :call/, err.message)
  end

  # @behavior S-150
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
    assert_raises(Kobako::UndefinedEntrypointError) { sandbox.run(:Missing) }
  end
end
