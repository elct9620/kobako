# frozen_string_literal: true

require "test_helper"

# E2E (Layer 4) — the preloaded snippet table through real mruby, source
# form: snippets replay in insertion order against every fresh mrb_state,
# and a snippet that will not compile or raises at replay surfaces as
# SandboxError. The binary form is test/e2e/test_preload_bytecode.rb.
class TestE2EPreload < Minitest::Test
  include E2eGuestHelper

  # The snippet table re-runs against the fresh mrb_state on every #eval,
  # not just once.
  # @behavior S-052
  def test_preloaded_snippet_is_visible_to_eval
    sandbox = Kobako::Sandbox.new
    sandbox.preload(code: "ANSWER = 42", name: :Answers)

    assert_equal 42, sandbox.eval("ANSWER").value
  end

  # @behavior S-053
  def test_preloaded_snippets_replay_across_invocations
    sandbox = Kobako::Sandbox.new
    sandbox.preload(code: "ANSWER = 42", name: :Answers)

    assert_equal 42, sandbox.eval("ANSWER").value
    assert_equal 42, sandbox.eval("ANSWER").value
  end

  # @behavior S-054
  def test_preloaded_snippets_replay_in_insertion_order
    sandbox = Kobako::Sandbox.new
    sandbox.preload(code: "BASE = 10", name: :Alpha)
    sandbox.preload(code: "EXTENDED = BASE * 2", name: :Beta)

    assert_equal 20, sandbox.eval("EXTENDED").value
  end

  # @behavior S-083 S-084
  # Accepting the snippet keeps the detection timing uniform with the
  # binary: form, which cannot be compiled at preload at all. Compilation
  # runs no snippet code, so the failure carries an empty backtrace.
  def test_snippet_compile_failure_surfaces_on_first_invocation_replay
    sandbox = Kobako::Sandbox.new
    assert_same sandbox, sandbox.preload(code: "def broken(", name: :Broken),
                "an uncompilable code: snippet through #preload must register without raising"

    err = assert_raises(Kobako::SandboxError) { sandbox.eval("nil") }
    assert_equal "sandbox", err.origin
    assert_match(/syntax error/, err.message,
                 "a snippet compile failure through the first #eval must surface " \
                 "the guest's syntax error")
  end

  # A compile runs nothing, so there is no backtrace to point at the
  # snippet; the message is the only place the Host App can read where
  # the parse stopped.
  # @behavior S-125
  def test_snippet_compile_failure_names_where_the_parse_stopped
    sandbox = Kobako::Sandbox.new
    sandbox.preload(code: "x = 1\ndef broken(", name: :Broken)

    err = assert_raises(Kobako::SandboxError) { sandbox.eval("nil") }
    assert_match(/\A\(snippet:Broken\):2:\d+: syntax error/, err.message,
                 "an uncompilable snippet through the first #eval must fail with a message " \
                 "naming the snippet and the line and column the parse stopped at")
  end

  # @behavior S-088
  # The backtrace has to name the snippet rather than the invocation that
  # replayed it, or a Host App reading the failure would look for the
  # raise in the source it just submitted.
  def test_preloaded_snippet_replay_failure_surfaces_as_sandbox_error
    sandbox = Kobako::Sandbox.new
    sandbox.preload(code: 'raise "broken at preload"', name: :Broken)

    err = assert_raises(Kobako::SandboxError) { sandbox.eval("nil") }
    assert_match(/broken at preload/, err.message)
    assert err.backtrace_lines.any? { |line| line.include?("(snippet:Broken)") },
           "expected backtrace to reference (snippet:Broken), got #{err.backtrace_lines.inspect}"
  end
end
