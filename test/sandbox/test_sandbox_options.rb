# frozen_string_literal: true

require "test_helper"

# Kobako::SandboxOptions normalises the four per-Sandbox caps. Pure Ruby —
# no native ext — so it runs on a clean checkout. The contract
# (docs/behavior/lifecycle.md B-01): an absent cap takes its DEFAULT, an
# explicit nil disables that bound, and a set output / memory cap must be a
# positive Integer. All four caps behave uniformly.
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
end
