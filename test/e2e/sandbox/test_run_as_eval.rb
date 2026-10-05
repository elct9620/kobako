# frozen_string_literal: true

require "test_helper"

# E2E — an entrypoint run held to what an evaluation promises, through
# real mruby. #run reaches guest code by a preloaded constant rather than
# by source, so every promise an evaluation makes — caps, captures, the
# seal — has to hold on this path too, and the backtrace is the one place
# the two are meant to differ.
class TestE2ERunAsEval < Minitest::Test
  include E2eGuestHelper

  # @behavior S-159
  def test_an_evaluation_failure_names_its_source_as_eval
    err = assert_raises(Kobako::SandboxError) { Kobako::Sandbox.new(wasm_path: REAL_WASM).eval("raise 'boom'") }

    assert(err.backtrace_lines.any? { |line| line.include?("(eval)") },
           "a failure raised by source through #eval must name that source as (eval), " \
           "got #{err.backtrace_lines.inspect}")
  end

  # @behavior S-160
  def test_an_entrypoint_failure_names_the_entrypoints_snippet_not_eval
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.preload(code: "Worker = ->(*_) { raise 'boom' }", name: :Worker)

    lines = assert_raises(Kobako::SandboxError) { sandbox.run(:Worker) }.backtrace_lines

    assert_equal [false, true], [lines.any? do |line|
      line.include?("(eval)")
    end, lines.last.include?("(snippet:Worker)")],
                 "a failure under #run must carry no (eval) frame and end on the entrypoint's " \
                 "snippet, got #{lines.inspect}"
  end

  # @behavior S-161
  def test_the_deadline_and_memory_budget_bound_a_run
    { timeout: [Kobako::TimeoutError, "Worker = ->(*_) { loop {} }"],
      memory_limit: [Kobako::MemoryLimitError, "Worker = ->(*_) { 'a' * 1_000_000 }"] }.each do |cap, (error, code)|
      sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM, timeout: 0.05, memory_limit: 1 << 20)
      sandbox.preload(code: code, name: :Worker)

      assert_raises(error, "#{cap} must bound an entrypoint run through #run as it bounds an evaluation") do
        sandbox.run(:Worker)
      end
    end
  end

  # @behavior S-162
  def test_a_run_captures_both_channels
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.preload(code: "Worker = ->(*_) { puts 'out'; $stderr.puts 'err'; 1 }", name: :Worker)

    execution = sandbox.run(:Worker)

    assert_equal %W[out\n err\n], [execution.stdout, execution.stderr],
                 "an entrypoint run through #run must capture both channels as an evaluation does"
  end

  # Each answers the Symbol naming its own shape, so the run reached it.
  ENTRYPOINTS = {
    module: "module Worker; def self.call(*) = :module; end",
    instance: "Worker = Class.new { def call(*) = :instance }.new"
  }.freeze

  # @behavior S-163
  def test_a_module_or_instance_answering_call_is_an_entrypoint
    ENTRYPOINTS.each do |shape, code|
      sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
      sandbox.preload(code: code, name: :Worker)

      assert_equal shape, sandbox.run(:Worker).value,
                   "a #{shape} answering call must be as valid a #run entrypoint as a Proc or a class"
    end
  end

  # @behavior S-164
  def test_a_run_as_the_first_invocation_seals_registration
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.preload(code: "Worker = ->(*_) { 1 }", name: :Worker)
    sandbox.run(:Worker)

    assert_raises(ArgumentError, "a bind after a first invocation made through #run must be refused") do
      sandbox.bind("Late", -> { 1 })
    end
  end

  # @behavior S-165
  def test_an_entrypoint_returning_a_value_the_wire_cannot_carry_is_a_sandbox_failure
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.preload(code: "Worker = ->(*_) { Object.new }", name: :Worker)

    assert_raises(Kobako::SandboxError,
                  "an entrypoint whose call returns a value the wire cannot carry must fail " \
                  "#run as a Sandbox failure") do
      sandbox.run(:Worker)
    end
  end
end
