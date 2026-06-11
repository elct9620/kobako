# frozen_string_literal: true

require "test_helper"

# E2E (Layer 4) — the preloaded snippet table through real mruby: source
# and binary (RITE bytecode) snippets replay in insertion order against
# every fresh mrb_state (B-32), and the replay / structural failure modes
# surface as SandboxError (E-36) or BytecodeError (E-37 / E-38).
class TestE2EPreload < Minitest::Test
  include E2eGuestHelper

  # B-32: preloaded snippets replay in insertion order against the fresh
  # mrb_state before each invocation. The first snippet defines a top-
  # level constant; subsequent invocations on the same Sandbox observe
  # it because the snippet table re-runs on every #eval, not just once.
  def test_b32_preloaded_snippet_is_visible_to_eval
    sandbox = Kobako::Sandbox.new
    sandbox.preload(code: "ANSWER = 42", name: :Answers)

    assert_equal 42, sandbox.eval("ANSWER")
  end

  def test_b32_preloaded_snippets_replay_across_invocations
    sandbox = Kobako::Sandbox.new
    sandbox.preload(code: "ANSWER = 42", name: :Answers)

    assert_equal 42, sandbox.eval("ANSWER")
    assert_equal 42, sandbox.eval("ANSWER")
  end

  def test_b32_preloaded_snippets_replay_in_insertion_order
    sandbox = Kobako::Sandbox.new
    sandbox.preload(code: "BASE = 10", name: :Alpha)
    sandbox.preload(code: "EXTENDED = BASE * 2", name: :Beta)

    assert_equal 20, sandbox.eval("EXTENDED")
  end

  # E-36: a preloaded snippet whose top-level expression raises during
  # replay surfaces as Kobako::SandboxError with the backtrace attributed
  # to the snippet's `(snippet:Name)` filename.
  def test_e36_preloaded_snippet_replay_failure_surfaces_as_sandbox_error
    sandbox = Kobako::Sandbox.new
    sandbox.preload(code: 'raise "broken at preload"', name: :Broken)

    err = assert_raises(Kobako::SandboxError) { sandbox.eval("nil") }
    assert_match(/broken at preload/, err.message)
    assert err.backtrace_lines.any? { |line| line.include?("(snippet:Broken)") },
           "expected backtrace to reference (snippet:Broken), got #{err.backtrace_lines.inspect}"
  end

  # docs/behavior.md B-32 (binary: form): a precompiled RITE bytecode
  # blob registered via `#preload(binary:)` is replayed against the
  # fresh `mrb_state` before each invocation, exactly like a `code:`
  # form snippet. The constant defined by the bytecode is observable to
  # subsequent `#eval` calls.
  #
  # Fixture source: `test/fixtures/snippet_answers.rb` (literally
  # `ANSWERS = 42`), compiled with `mrbc -g` to embed a `debug_info`
  # section so the bytecode meets B-32's identity requirement.
  BYTECODE_FIXTURE_PATH = File.expand_path("../fixtures/snippet_answers.mrb", __dir__)

  def test_b32_preloaded_binary_snippet_is_visible_to_eval
    sandbox = Kobako::Sandbox.new
    sandbox.preload(binary: File.binread(BYTECODE_FIXTURE_PATH))

    assert_equal 42, sandbox.eval("ANSWERS"),
                 "B-32 (binary: form): preloaded bytecode must contribute its " \
                 "top-level constants to subsequent #eval calls"
  end

  def test_b32_preloaded_binary_snippet_replays_across_invocations
    sandbox = Kobako::Sandbox.new
    sandbox.preload(binary: File.binread(BYTECODE_FIXTURE_PATH))

    assert_equal 42, sandbox.eval("ANSWERS")
    assert_equal 42, sandbox.eval("ANSWERS"),
                 "B-32: bytecode snippet must replay against every fresh mrb_state, " \
                 "not just the first invocation"
  end

  # docs/behavior.md E-37: bytecode whose RITE version mismatches the
  # guest's pinned version surfaces as Kobako::BytecodeError on the
  # first invocation's snippet replay. The wrong_version fixture takes
  # the valid bytecode and flips the version bytes ("0400" → "9999")
  # so the failure path triggers without depending on a future mruby
  # version bump.
  E37_FIXTURE_PATH = File.expand_path("../fixtures/snippet_wrong_version.mrb", __dir__)

  def test_e37_bytecode_wrong_version_raises_bytecode_error
    sandbox = Kobako::Sandbox.new
    sandbox.preload(binary: File.binread(E37_FIXTURE_PATH))

    err = assert_raises(Kobako::BytecodeError) { sandbox.eval("nil") }
    assert_kind_of Kobako::SandboxError, err,
                   "BytecodeError must remain a SandboxError subclass"
    assert_equal "sandbox", err.origin
    assert_equal "Kobako::BytecodeError", err.klass
  end

  # docs/behavior.md E-38: bytecode body that fails structural parse
  # against the loaded IREP reader surfaces as Kobako::BytecodeError.
  # The corrupt fixture is a header-prefix truncation of the valid
  # bytecode — enough to pass the four-byte RITE ident check but short
  # enough that section parsing fails inside mruby's load path.
  E38_FIXTURE_PATH = File.expand_path("../fixtures/snippet_corrupt.mrb", __dir__)

  def test_e38_bytecode_corrupt_body_raises_bytecode_error
    sandbox = Kobako::Sandbox.new
    sandbox.preload(binary: File.binread(E38_FIXTURE_PATH))

    err = assert_raises(Kobako::BytecodeError) { sandbox.eval("nil") }
    assert_kind_of Kobako::SandboxError, err
    assert_equal "Kobako::BytecodeError", err.klass
  end

  # docs/behavior.md E-36 (binary: form): bytecode that loads cleanly
  # but whose top-level expression raises at replay surfaces as
  # Kobako::SandboxError with the natural mruby class preserved — NOT
  # promoted to Kobako::BytecodeError, which is reserved for the two
  # structural failure modes (E-37 / E-38). The raise_boom fixture is
  # `raise "boom from snippet"` compiled with `mrbc -g`.
  #
  # Scope: this test pins the E-36 dispatch contract only — E-36 covers
  # binary form, and the regression risk is a silent unconditional
  # promotion to BytecodeError. Backtrace attribution for
  # binary form (whatever filename the bytecode's debug_info carries,
  # routed through mruby's own `pack_backtrace`) is upstream-inherited,
  # so it is not separately pinned here. The source-form companion at
  # `test_e36_preloaded_snippet_replay_failure_surfaces_as_sandbox_error`
  # exercises the parallel attribution path for the `(snippet:Name)`
  # ccontext filename, which is host-set rather than upstream-inherited.
  E36_BINARY_FIXTURE_PATH = File.expand_path("../fixtures/snippet_raise_boom.mrb", __dir__)

  def test_e36_binary_form_replay_raise_is_sandbox_error_not_bytecode_error
    sandbox = Kobako::Sandbox.new
    sandbox.preload(binary: File.binread(E36_BINARY_FIXTURE_PATH))

    err = assert_raises(Kobako::SandboxError) { sandbox.eval("nil") }
    refute_kind_of Kobako::BytecodeError, err,
                   "E-36: a binary-form snippet that raises at top level is " \
                   "a replay failure, not a bytecode structural failure"
    assert_equal "RuntimeError", err.klass,
                 "E-36: the natural mruby exception class must survive replay"
    assert_equal "sandbox", err.origin
    assert_match(/boom from snippet/, err.message)
  end

  # docs/behavior.md B-32 (binary: form): bytecode emitted without
  # `mrbc -g` carries no `debug_info` section. Per the relaxed B-32 it
  # remains a legal payload — the guest loads it normally and the
  # snippet contributes its top-level effects to the fresh `mrb_state`.
  # Backtrace frames originating in the snippet are silently omitted
  # per upstream mruby semantics, but class / message / origin
  # attribution on raised exceptions remain intact. The no_debug
  # fixture is the same `ANSWERS = 42` source compiled with the debug
  # switch omitted.
  STRIPPED_BYTECODE_FIXTURE_PATH = File.expand_path("../fixtures/snippet_no_debug.mrb", __dir__)

  def test_b32_stripped_bytecode_loads_and_contributes_top_level_effects
    sandbox = Kobako::Sandbox.new
    sandbox.preload(binary: File.binread(STRIPPED_BYTECODE_FIXTURE_PATH))

    assert_equal 42, sandbox.eval("ANSWERS"),
                 "B-32: bytecode without debug_info must still contribute " \
                 "top-level effects on the fresh mrb_state"
  end
end
