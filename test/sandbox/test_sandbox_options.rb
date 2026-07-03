# frozen_string_literal: true

require "test_helper"

# Kobako::SandboxOptions normalises the four per-Sandbox caps and the
# isolation-profile floor, and owns the PROFILES ladder comparison
# (#enforce_floor!) that Sandbox.new delegates its construction floor
# check to. Pure Ruby — no native ext — so it runs on a clean checkout.
# The contract (docs/behavior/lifecycle.md B-01,
# docs/behavior/security.md B-54): an absent cap takes its DEFAULT, an
# explicit nil disables that bound, and a set output / memory cap must be a
# positive Integer. All four caps behave uniformly. The profile option is
# the one non-cap: nil is NOT a disable switch there — the no-floor
# request is an explicit :permissive — so nil is rejected with the other
# non-ladder values (E-39).
class TestSandboxOptions < Minitest::Test
  def test_absent_caps_take_their_defaults
    options = Kobako::SandboxOptions.new

    assert_equal Kobako::SandboxOptions::DEFAULT_TIMEOUT_SECONDS, options.timeout
    assert_equal Kobako::SandboxOptions::DEFAULT_MEMORY_LIMIT, options.memory_limit
    assert_equal Kobako::SandboxOptions::DEFAULT_OUTPUT_LIMIT, options.stdout_limit
    assert_equal Kobako::SandboxOptions::DEFAULT_OUTPUT_LIMIT, options.stderr_limit
  end

  def test_explicit_nil_disables_each_cap
    options = Kobako::SandboxOptions.new(timeout: nil, memory_limit: nil,
                                         stdout_limit: nil, stderr_limit: nil)

    assert_nil options.timeout, "an explicit nil timeout must disable the wall-clock bound"
    assert_nil options.memory_limit, "an explicit nil memory_limit must disable the memory bound"
    assert_nil options.stdout_limit, "an explicit nil stdout_limit must leave stdout uncapped"
    assert_nil options.stderr_limit, "an explicit nil stderr_limit must leave stderr uncapped"
  end

  def test_positive_output_limits_pass_through
    options = Kobako::SandboxOptions.new(stdout_limit: 100, stderr_limit: 200)

    assert_equal 100, options.stdout_limit
    assert_equal 200, options.stderr_limit
  end

  def test_rejects_zero_or_negative_output_limit
    [0, -1].each do |bad|
      assert_raises(ArgumentError, "stdout_limit #{bad.inspect} must be rejected as not a positive Integer") do
        Kobako::SandboxOptions.new(stdout_limit: bad)
      end
      assert_raises(ArgumentError, "stderr_limit #{bad.inspect} must be rejected as not a positive Integer") do
        Kobako::SandboxOptions.new(stderr_limit: bad)
      end
    end
  end

  def test_rejects_non_integer_output_limit
    [1.5, "100"].each do |bad|
      assert_raises(ArgumentError, "stdout_limit #{bad.inspect} must be rejected as not an Integer") do
        Kobako::SandboxOptions.new(stdout_limit: bad)
      end
      assert_raises(ArgumentError, "stderr_limit #{bad.inspect} must be rejected as not an Integer") do
        Kobako::SandboxOptions.new(stderr_limit: bad)
      end
    end
  end

  def test_absent_profile_takes_the_hermetic_default
    assert_equal :hermetic, Kobako::SandboxOptions.new.profile,
                 "an absent profile through SandboxOptions.new must default to the strictest floor, :hermetic"
  end

  def test_ladder_profiles_pass_through
    Kobako::SandboxOptions::PROFILES.each do |profile|
      assert_equal profile, Kobako::SandboxOptions.new(profile: profile).profile,
                   "ladder value #{profile.inspect} through SandboxOptions.new must be readable back unchanged"
    end
  end

  def test_rejects_profile_outside_the_ladder
    # nil included deliberately: the no-floor request is an explicit
    # :permissive, so profile has no nil-disable form (B-54 / E-39).
    [nil, :sealed, "hermetic", 1].each do |bad|
      assert_raises(ArgumentError, "profile #{bad.inspect} through SandboxOptions.new must be rejected") do
        Kobako::SandboxOptions.new(profile: bad)
      end
    end
  end

  # The floor check's failing branch (E-49) is witnessed here, on the
  # ladder owner, with a plain declared value — the bundled runtime
  # always builds the requested rung, so no real runtime can hand
  # Sandbox.new a below-floor declaration.
  def test_enforce_floor_rejects_a_declaration_below_the_requested_floor
    err = assert_raises(Kobako::SetupError,
                        "a :permissive declaration through #enforce_floor! must fail a :hermetic floor (E-49)") do
      Kobako::SandboxOptions.new(profile: :hermetic).enforce_floor!(:permissive)
    end
    assert_match(/permissive/, err.message)
    assert_match(/hermetic/, err.message)
  end

  # B-54's fail-closed clause: a declaration the gem cannot place on the
  # ladder ranks below every floor. Witnessed against the :permissive
  # floor because that is the rung an off-ladder declaration could most
  # plausibly slip past.
  def test_enforce_floor_ranks_an_off_ladder_declaration_below_every_floor
    err = assert_raises(Kobako::SetupError,
                        "an off-ladder declaration must rank below even the :permissive floor (B-54 fail-closed)") do
      Kobako::SandboxOptions.new(profile: :permissive).enforce_floor!(:isolated)
    end
    assert_match(/isolated/, err.message)
  end

  # B-54's passing branch: a declaration at the floor constructs, and a
  # runtime that can only build a stronger posture satisfies a weaker
  # request by declaring what it built.
  def test_enforce_floor_accepts_a_declaration_at_or_above_the_floor
    assert_nil Kobako::SandboxOptions.new(profile: :hermetic).enforce_floor!(:hermetic),
               "a :hermetic declaration through #enforce_floor! must pass the :hermetic floor"
    assert_nil Kobako::SandboxOptions.new(profile: :permissive).enforce_floor!(:hermetic),
               "a :hermetic declaration through #enforce_floor! must satisfy the weaker :permissive floor"
  end
end
