# frozen_string_literal: true

require "test_helper"

# E2E (Layer 4) — the preloaded snippet table through real mruby, binary
# (RITE bytecode) form: bytecode replays against every fresh mrb_state
# like a source snippet, a blob that fails its structural check surfaces as
# BytecodeError, and a program that loads and then raises keeps the class
# it raised. The source form is test/e2e/test_preload.rb.
class TestE2EPreloadBytecode < Minitest::Test
  include E2eGuestHelper

  # Fixture source: `test/fixtures/snippet_answers.rb` (literally
  # `ANSWERS = 42`), compiled with `mrbc -g` to embed a `debug_info` section.
  BYTECODE_FIXTURE_PATH = TestPaths.fixture("snippet_answers.mrb")

  # @behavior S-055
  def test_preloaded_binary_snippet_is_visible_to_eval
    sandbox = Kobako::Sandbox.new
    sandbox.preload(binary: File.binread(BYTECODE_FIXTURE_PATH))

    assert_equal 42, sandbox.eval("ANSWERS").value,
                 "preloaded bytecode must contribute its " \
                 "top-level constants to subsequent #eval calls"
  end

  # @behavior S-056
  def test_preloaded_binary_snippet_replays_across_invocations
    sandbox = Kobako::Sandbox.new
    sandbox.preload(binary: File.binread(BYTECODE_FIXTURE_PATH))

    assert_equal 42, sandbox.eval("ANSWERS").value
    assert_equal 42, sandbox.eval("ANSWERS").value,
                 "bytecode snippet must replay against every fresh mrb_state, " \
                 "not just the first invocation"
  end

  # The wrong_version fixture flips the valid bytecode's version bytes
  # ("0400" → "9999") so the failure path triggers without depending on a
  # future mruby version bump.
  WRONG_VERSION_FIXTURE_PATH = TestPaths.fixture("snippet_wrong_version.mrb")

  # @behavior S-148
  def test_bytecode_wrong_version_raises_bytecode_error
    sandbox = Kobako::Sandbox.new
    sandbox.preload(binary: File.binread(WRONG_VERSION_FIXTURE_PATH))

    err = assert_raises(Kobako::BytecodeError) { sandbox.eval("nil") }
    assert_kind_of Kobako::SandboxError, err,
                   "BytecodeError must remain a SandboxError subclass"
    assert_equal "sandbox", err.origin
    assert_equal "Kobako::BytecodeError", err.klass
  end

  # The corrupt fixture is a header-prefix truncation of the valid
  # bytecode — enough to pass the four-byte RITE ident check but short
  # enough that section parsing fails inside mruby's load path.
  CORRUPT_FIXTURE_PATH = TestPaths.fixture("snippet_corrupt.mrb")

  # @behavior S-089
  def test_bytecode_corrupt_body_raises_bytecode_error
    sandbox = Kobako::Sandbox.new
    sandbox.preload(binary: File.binread(CORRUPT_FIXTURE_PATH))

    err = assert_raises(Kobako::BytecodeError) { sandbox.eval("nil") }
    assert_kind_of Kobako::SandboxError, err
    assert_equal "Kobako::BytecodeError", err.klass
  end

  # The regression risk is a silent unconditional promotion to
  # BytecodeError, which is reserved for the structural failures. The
  # raise_boom fixture is `raise "boom from snippet"` compiled with
  # `mrbc -g`.
  #
  # Backtrace attribution for the binary form is whatever filename the
  # bytecode's debug_info carries, routed through mruby's own
  # `pack_backtrace`, so it is upstream-inherited and not pinned here;
  # the source-form companion in test/e2e/test_preload.rb exercises the
  # host-set `(snippet:Name)` filename instead.
  RAISE_BOOM_FIXTURE_PATH = TestPaths.fixture("snippet_raise_boom.mrb")

  # @behavior S-090
  def test_binary_form_replay_raise_is_sandbox_error_not_bytecode_error
    sandbox = Kobako::Sandbox.new
    sandbox.preload(binary: File.binread(RAISE_BOOM_FIXTURE_PATH))

    err = assert_raises(Kobako::SandboxError) { sandbox.eval("nil") }
    refute_kind_of Kobako::BytecodeError, err,
                   "a binary-form snippet that raises at top level is " \
                   "a replay failure, not a bytecode structural failure"
    assert_equal "RuntimeError", err.klass,
                 "the natural mruby exception class must survive replay"
    assert_equal "sandbox", err.origin
    assert_match(/boom from snippet/, err.message)
  end

  # A load that fails its structural check answers with ScriptError
  # itself, so that one class is what marks a structural failure; a
  # subclass the program raises is its own failure. The two fixtures are
  # `raise ScriptError` and `raise NotImplementedError` compiled with
  # `mrbc -g`.
  SCRIPT_ERROR_FIXTURE_PATH = TestPaths.fixture("snippet_raise_script_error.mrb")
  SCRIPT_ERROR_SUBCLASS_FIXTURE_PATH = TestPaths.fixture("snippet_raise_not_implemented.mrb")

  # @behavior S-126
  def test_binary_form_raising_script_error_itself_reads_as_bytecode_error
    sandbox = Kobako::Sandbox.new
    sandbox.preload(binary: File.binread(SCRIPT_ERROR_FIXTURE_PATH))

    err = assert_raises(Kobako::BytecodeError) { sandbox.eval("nil") }
    assert_equal "Kobako::BytecodeError", err.klass,
                 "bytecode raising ScriptError itself through the first #eval must " \
                 "fail as the structural failure that class marks"
  end

  # @behavior S-090
  def test_binary_form_raising_a_script_error_subclass_keeps_its_class
    sandbox = Kobako::Sandbox.new
    sandbox.preload(binary: File.binread(SCRIPT_ERROR_SUBCLASS_FIXTURE_PATH))

    err = assert_raises(Kobako::SandboxError) { sandbox.eval("nil") }
    assert_equal "NotImplementedError", err.klass,
                 "bytecode raising a ScriptError subclass through the first #eval must " \
                 "fail carrying that subclass, not as a structural failure"
  end

  # Bytecode emitted without `mrbc -g` carries no `debug_info` section yet
  # remains a legal payload; only the snippet's backtrace frames are omitted,
  # per upstream mruby. The no_debug fixture is the same `ANSWERS = 42`
  # source compiled with the debug switch omitted.
  STRIPPED_BYTECODE_FIXTURE_PATH = TestPaths.fixture("snippet_no_debug.mrb")

  # @behavior S-057
  def test_stripped_bytecode_loads_and_contributes_top_level_effects
    sandbox = Kobako::Sandbox.new
    sandbox.preload(binary: File.binread(STRIPPED_BYTECODE_FIXTURE_PATH))

    assert_equal 42, sandbox.eval("ANSWERS").value,
                 "bytecode without debug_info must still contribute " \
                 "top-level effects on the fresh mrb_state"
  end
end
