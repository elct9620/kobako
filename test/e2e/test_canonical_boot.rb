# frozen_string_literal: true

require "test_helper"

# E2E (Layer 4) — the canonical boot state (docs/behavior/runtime.md B-49)
# through real mruby: every invocation observes the deterministic
# post-boot interpreter state, identical across invocations and
# carrying no artifact of prior ones. The heap-layout witness below is
# what distinguishes B-49 from plain B-03 isolation — not merely "no
# leak", but "byte-identical starting state".
class TestE2ECanonicalBoot < Minitest::Test
  include E2eGuestHelper

  def setup
    @sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
  end

  # B-49 + B-45: with an identical starting state and no ambient
  # entropy, the first allocation of each invocation lands on the same
  # heap slot — the object_id sequence replays exactly.
  def test_first_allocation_object_id_replays_across_invocations
    first = @sandbox.eval("Object.new.object_id")
    second = @sandbox.eval("Object.new.object_id")

    assert_equal first, second,
                 "the first Object.new.object_id through repeated #eval must replay identically (B-49)"
  end

  # B-03 / B-49 on the #eval verb (the #run twin lives in
  # test_lifecycle.rb): a guest global set by one #eval is unset at the
  # next #eval's entry.
  def test_eval_does_not_leak_guest_globals_between_invocations
    first = @sandbox.eval("s = $leak; $leak = true; s")
    second = @sandbox.eval("s = $leak; $leak = true; s")

    assert_nil first, "the first #eval on a fresh Sandbox must observe an unset guest global (B-49)"
    assert_nil second, "a repeated #eval must not surface the prior invocation's guest global (B-03/B-49)"
  end

  # B-49: constants defined by one invocation's source do not exist at
  # the next invocation's entry — class definitions are invocation-local
  # unless preloaded (B-32).
  def test_eval_defined_constants_do_not_survive_invocations
    @sandbox.eval("class CanonicalBootProbe; end; true")

    refute @sandbox.eval("Object.const_defined?(:CanonicalBootProbe)"),
           "a constant defined by a prior #eval must not exist at the next invocation's entry (B-49)"
  end
end
