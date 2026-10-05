# frozen_string_literal: true

require "test_helper"

# E2E — what a preloaded snippet's failure says about where it happened,
# through real mruby. A snippet that ran reports the frames its own form
# carries — the filename a compiler embedded, or none when it embedded no
# debug information — and one that never loaded has no frame to report.
class TestE2EPreloadFailureFrames < Minitest::Test
  include E2eGuestHelper

  # `raise "boom from snippet"`, compiled with and without `mrbc -g`.
  RAISE_BOOM = TestPaths.fixture("snippet_raise_boom.mrb")
  RAISE_BOOM_NO_DEBUG = TestPaths.fixture("snippet_raise_boom_no_debug.mrb")

  # Each fails before any of the snippet runs: one does not compile, one
  # names another format version, one is cut short.
  UNLOADABLE = {
    "source that will not compile" => { code: "def broken(", name: :Broken },
    "bytecode for another format version" => { binary: File.binread(TestPaths.fixture("snippet_wrong_version.mrb")) },
    "a corrupt bytecode body" => { binary: File.binread(TestPaths.fixture("snippet_corrupt.mrb")) }
  }.freeze

  # @behavior S-155
  def test_bytecode_frames_carry_the_filename_its_compiler_embedded
    err = replay_failure(binary: File.binread(RAISE_BOOM))

    assert(err.backtrace_lines.any? { |line| line.include?("snippet_raise_boom.rb") },
           "a raise from bytecode through #eval must report frames under the filename mrbc " \
           "embedded, got #{err.backtrace_lines.inspect}")
  end

  # @behavior S-156
  def test_a_snippet_that_never_loaded_fails_with_an_empty_backtrace
    UNLOADABLE.each do |form, preload|
      assert_empty replay_failure(**preload).backtrace_lines,
                   "#{form} preloaded and replayed through #eval must fail with an empty " \
                   "backtrace, since none of it ran"
    end
  end

  # @behavior S-157
  def test_bytecode_failing_its_structural_check_fails_every_invocation_alike
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.preload(**UNLOADABLE.fetch("a corrupt bytecode body"))

    failures = Array.new(2) { assert_raises(Kobako::BytecodeError) { sandbox.eval("nil") } }

    assert_equal failures.first.message, failures.last.message,
                 "bytecode failing its structural check must fail every later #eval the same way"
  end

  # @behavior S-158
  def test_bytecode_without_debug_information_keeps_class_message_and_origin
    err = replay_failure(binary: File.binread(RAISE_BOOM_NO_DEBUG))

    assert_equal ["RuntimeError", "boom from snippet", "sandbox"],
                 [err.klass, err.message[/boom from snippet/], err.origin],
                 "a raise from bytecode with no debug information through #eval must keep its " \
                 "class, message and origin"
    refute(err.backtrace_lines.any? { |line| line.include?("snippet_raise_boom.rb") },
           "bytecode with no debug information must leave the snippet's frames out of the backtrace")
  end

  private

  def replay_failure(**preload)
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.preload(**preload)
    assert_raises(Kobako::SandboxError) { sandbox.eval("nil") }
  end
end
