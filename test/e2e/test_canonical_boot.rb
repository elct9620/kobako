# frozen_string_literal: true

require "test_helper"

# E2E — the canonical boot state through real mruby: every
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

  # @behavior MR-009
  # The permissive profile leaves time and entropy live for guest code, but
  # the state an invocation starts from is fixed before either is read.
  def test_first_allocation_replays_under_the_permissive_profile_too
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM, profile: :permissive)
    first = sandbox.eval("Object.new.object_id").value
    second = sandbox.eval("Object.new.object_id").value

    assert_equal first, second,
                 "the first Object.new.object_id through repeated #eval on a permissive Sandbox " \
                 "must replay identically, as it does under the hermetic profile"
  end

  # The default artifact composes no gem that reads a clock or an entropy
  # source, so guest code finds no name to reach either through. +puts+ is
  # the control that shows the probe can answer true at all.
  AMBIENT_SURFACE_PROBE = <<~RUBY
    [Kernel.respond_to?(:puts, true), Object.const_defined?(:Time), Object.const_defined?(:Random),
     Kernel.respond_to?(:sleep, true), Kernel.respond_to?(:rand, true)]
  RUBY

  # @behavior MR-010
  def test_the_default_guest_defines_no_time_sleep_or_randomness_surface
    assert_equal [true, false, false, false, false], @sandbox.eval(AMBIENT_SURFACE_PROBE).value,
                 "the default Guest Binary through #eval must define no Time, Random, sleep, or rand"
  end

  # @behavior S-154
  # The failed run raised after writing, so its write is the one a leak
  # would carry into the next entry.
  def test_a_failed_invocation_leaves_nothing_for_the_next_entry
    assert_raises(Kobako::SandboxError) { @sandbox.eval("$leak = :left_behind; raise 'fail'") }

    assert_nil @sandbox.eval("$leak").value,
               "a guest global written by a failed #eval must be gone at the next invocation's entry"
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

  # @behavior S-018
  # The guest materializes its constants from the declared path set, so a
  # refused late bind shows up as a name the next invocation cannot reach.
  def test_a_path_refused_after_the_seal_stays_out_of_the_declared_set
    @sandbox.bind("MyService::KV", -> { "kv" })
    @sandbox.eval("1")
    assert_raises(ArgumentError) { @sandbox.bind("MyService::Late", -> { "late" }) }

    result = @sandbox.eval("[MyService::KV.call, MyService.const_defined?(:Late)]").value

    assert_equal ["kv", false], result,
                 "a bind refused after the first #eval must leave the next invocation's declared " \
                 "path set as it was at the seal"
  end
end
