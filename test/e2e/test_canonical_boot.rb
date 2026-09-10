# frozen_string_literal: true

require "test_helper"

# E2E (Layer 4) — the canonical boot state through real mruby: every
# invocation observes the deterministic post-boot interpreter state,
# identical across invocations and carrying no artifact of prior ones. The
# heap-layout witness below is what distinguishes it from plain invocation
# isolation — not merely "no leak", but "byte-identical starting state".
class TestE2ECanonicalBoot < Minitest::Test
  include E2eGuestHelper

  def setup
    super
    @sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
  end

  # @behavior MR-001
  # With an identical starting state and no ambient entropy, the first
  # allocation of each invocation lands on the same heap slot.
  def test_first_allocation_object_id_replays_across_invocations
    first = @sandbox.eval("Object.new.object_id").value
    second = @sandbox.eval("Object.new.object_id").value

    assert_equal first, second,
                 "the first Object.new.object_id through repeated #eval must replay identically"
  end

  # @behavior S-013
  # The #eval twin of the #run witness in test_lifecycle.rb.
  def test_eval_does_not_leak_guest_globals_between_invocations
    first = @sandbox.eval("s = $leak; $leak = true; s").value
    second = @sandbox.eval("s = $leak; $leak = true; s").value

    assert_nil first, "the first #eval on a fresh Sandbox must observe an unset guest global"
    assert_nil second, "a repeated #eval must not surface the prior invocation's guest global"
  end

  # @behavior MR-002
  # Class definitions are invocation-local unless preloaded.
  def test_eval_defined_constants_do_not_survive_invocations
    @sandbox.eval("class CanonicalBootProbe; end; true").value

    refute @sandbox.eval("Object.const_defined?(:CanonicalBootProbe)").value,
           "a constant defined by a prior #eval must not exist at the next invocation's entry"
  end
end
