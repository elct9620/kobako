# frozen_string_literal: true

require "test_helper"

# Kobako::SandboxOptions normalises the four per-Sandbox caps and the
# isolation-profile floor, and owns the PROFILES ladder comparison
# (#enforce_floor!) that Sandbox.new delegates its construction floor
# check to. Pure Ruby — no native ext — so it runs on a clean checkout.
# The profile option is the one non-cap: nil is NOT a disable switch there
# — the no-floor request is an explicit :permissive.
class TestSandboxOptions < Minitest::Test
  # Pins the literal SPEC default values (60 s / 1 MiB), not just the
  # DEFAULT_* constants, so a drift in either direction is caught here.
  # @behavior S-004
  def test_absent_caps_take_their_spec_defaults
    options = Kobako::SandboxOptions.new

    assert_equal 60.0, options.timeout
    assert_equal 1 << 20, options.memory_limit
    assert_equal 1 << 20, options.stdout_limit
    assert_equal 1 << 20, options.stderr_limit
  end

  # @behavior S-005
  def test_explicit_nil_disables_each_cap
    options = Kobako::SandboxOptions.new(timeout: nil, memory_limit: nil,
                                         stdout_limit: nil, stderr_limit: nil)

    assert_nil options.timeout, "an explicit nil timeout must disable the wall-clock bound"
    assert_nil options.memory_limit, "an explicit nil memory_limit must disable the memory bound"
    assert_nil options.stdout_limit, "an explicit nil stdout_limit must leave stdout uncapped"
    assert_nil options.stderr_limit, "an explicit nil stderr_limit must leave stderr uncapped"
  end

  # @behavior S-006
  def test_set_caps_pass_through_normalized
    options = Kobako::SandboxOptions.new(timeout: 1.5, memory_limit: 2 << 20,
                                         stdout_limit: 100, stderr_limit: 200)

    assert_in_delta 1.5, options.timeout, 1e-9
    assert_equal 2 << 20, options.memory_limit
    assert_equal 100, options.stdout_limit
    assert_equal 200, options.stderr_limit
  end

  # @behavior S-007
  def test_integer_timeout_is_coerced_to_float
    options = Kobako::SandboxOptions.new(timeout: 5)

    assert_kind_of Float, options.timeout
    assert_equal 5.0, options.timeout
  end

  # timeout accepts any positive finite Numeric; the byte caps demand a
  # positive Integer — the matrix keeps the four caps' reject rules
  # pinned side by side so an asymmetry cannot creep in unnoticed.
  INVALID_CAP_VALUES = {
    timeout: [0, -1.0, "60"],
    memory_limit: [0, -1, 1.5, "100"],
    stdout_limit: [0, -1, 1.5, "100"],
    stderr_limit: [0, -1, 1.5, "100"]
  }.freeze

  # @behavior RT-058
  def test_rejects_invalid_cap_values
    INVALID_CAP_VALUES.each do |cap, values|
      values.each do |bad|
        assert_raises(ArgumentError, "#{cap} #{bad.inspect} through SandboxOptions.new must be rejected") do
          Kobako::SandboxOptions.new(cap => bad)
        end
      end
    end
  end

  # @behavior RT-009
  def test_absent_profile_takes_the_hermetic_default
    assert_equal :hermetic, Kobako::SandboxOptions.new.profile,
                 "an absent profile through SandboxOptions.new must default to the strictest floor, :hermetic"
  end

  # @behavior RT-010
  def test_ladder_profiles_pass_through
    Kobako::SandboxOptions::PROFILES.each do |profile|
      assert_equal profile, Kobako::SandboxOptions.new(profile: profile).profile,
                   "ladder value #{profile.inspect} through SandboxOptions.new must be readable back unchanged"
    end
  end

  # @behavior RT-011
  def test_rejects_profile_outside_the_ladder
    # nil included deliberately: the no-floor request is an explicit
    # :permissive, so profile has no nil-disable form.
    [nil, :sealed, "hermetic", 1].each do |bad|
      assert_raises(ArgumentError, "profile #{bad.inspect} through SandboxOptions.new must be rejected") do
        Kobako::SandboxOptions.new(profile: bad)
      end
    end
  end

  # @behavior RT-018
  def test_absent_gvl_takes_the_hold_default
    assert_equal :hold, Kobako::SandboxOptions.new.gvl,
                 "an absent gvl through SandboxOptions.new must default to :hold, the GVL-holding mode"
  end

  # @behavior RT-019
  def test_gvl_modes_pass_through
    Kobako::SandboxOptions::GVL_MODES.each do |mode|
      assert_equal mode, Kobako::SandboxOptions.new(gvl: mode).gvl,
                   "gvl mode #{mode.inspect} through SandboxOptions.new must be readable back unchanged"
    end
  end

  # @behavior RT-020
  def test_rejects_gvl_outside_the_mode_set
    # nil included deliberately: gvl is requested as an explicit mode, so
    # it has no nil-disable form — anything off GVL_MODES is rejected.
    [nil, :auto, "release", 1].each do |bad|
      assert_raises(ArgumentError, "gvl #{bad.inspect} through SandboxOptions.new must be rejected") do
        Kobako::SandboxOptions.new(gvl: bad)
      end
    end
  end

  # @behavior RT-012
  # The floor check's failing branch is witnessed here, on the
  # ladder owner, with a plain declared value — the bundled runtime
  # always builds the requested rung, so no real runtime can hand
  # Sandbox.new a below-floor declaration.
  def test_enforce_floor_rejects_a_declaration_below_the_requested_floor
    err = assert_raises(Kobako::SetupError,
                        "a :permissive declaration through #enforce_floor! must fail a :hermetic floor") do
      Kobako::SandboxOptions.new(profile: :hermetic).enforce_floor!(:permissive)
    end
    assert_match(/permissive/, err.message)
    assert_match(/hermetic/, err.message)
  end

  # @behavior RT-013
  # Fail-closed: a declaration the gem cannot place on the ladder ranks
  # below every floor. Witnessed against the :permissive
  # floor because that is the rung an off-ladder declaration could most
  # plausibly slip past.
  def test_enforce_floor_ranks_an_off_ladder_declaration_below_every_floor
    err = assert_raises(Kobako::SetupError,
                        "an off-ladder declaration must rank below even the :permissive floor, failing closed") do
      Kobako::SandboxOptions.new(profile: :permissive).enforce_floor!(:isolated)
    end
    assert_match(/isolated/, err.message)
  end

  # @behavior RT-014
  # The passing branch: a declaration at the floor constructs, and a
  # runtime that can only build a stronger posture satisfies a weaker
  # request by declaring what it built. The contract is "returns
  # without raising" — the return value is void — and minitest turns
  # any raise into a failure, so the bare calls are the whole witness.
  def test_enforce_floor_accepts_a_declaration_at_or_above_the_floor
    Kobako::SandboxOptions.new(profile: :hermetic).enforce_floor!(:hermetic)
    Kobako::SandboxOptions.new(profile: :permissive).enforce_floor!(:hermetic)
  end
end
