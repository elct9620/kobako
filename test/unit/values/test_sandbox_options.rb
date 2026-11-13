# frozen_string_literal: true

require "test_helper"

# Kobako::SandboxOptions holds the per-Sandbox caps as the runtime reads them,
# and the profile and gvl it was given; the runtime judges those two as
# Sandbox.new builds it.
class TestSandboxOptions < Minitest::Test
  # Every option SandboxOptions.new requires, so a test names only the one
  # it is about.
  GIVEN = { timeout: 60.0, setup_timeout: 10.0, memory_limit: 4 << 20, stdout_limit: 1 << 20,
            stderr_limit: 1 << 20, open_file_limit: 1024, open_connection_limit: 6, connection_limit: 50,
            profile: :hermetic, gvl: :hold }.freeze

  # @behavior S-005
  def test_explicit_nil_disables_each_cap
    caps = %i[timeout setup_timeout memory_limit stdout_limit stderr_limit open_file_limit open_connection_limit
              connection_limit].to_h { |cap| [cap, nil] }

    assert_equal caps, options(**caps).to_h.slice(*caps.keys),
                 "an explicit nil must switch each cap off, leaving its bound unenforced"
  end

  # @behavior S-006
  def test_set_caps_pass_through_normalized
    caps = { timeout: 1.5, memory_limit: 2 << 20, stdout_limit: 100, stderr_limit: 200, open_file_limit: 8,
             open_connection_limit: 3, connection_limit: 9 }

    assert_equal caps, options(**caps).to_h.slice(*caps.keys),
                 "each cap set on SandboxOptions.new must read back as it was given"
  end

  # @behavior S-007
  def test_integer_timeout_is_coerced_to_float
    timeout = options(timeout: 5).timeout

    assert_kind_of Float, timeout
    assert_equal 5.0, timeout
  end

  # timeout accepts any positive finite Numeric; the other caps demand a
  # positive Integer — the matrix keeps the caps' reject rules pinned side
  # by side so an asymmetry cannot creep in unnoticed. A deadline that
  # never arrives is refused too: the way to ask for none is nil.
  INVALID_CAP_VALUES = {
    timeout: [0, -1.0, "60", Float::INFINITY, Float::NAN, 1e300],
    memory_limit: [0, -1, 1.5, "100"],
    stdout_limit: [0, -1, 1.5, "100"],
    stderr_limit: [0, -1, 1.5, "100"],
    open_file_limit: [0, -1, 1.5, "100"],
    open_connection_limit: [0, -1, 1.5, "100"],
    connection_limit: [0, -1, 1.5, "100"]
  }.freeze

  # @behavior RT-058 RT-061
  def test_rejects_invalid_cap_values
    INVALID_CAP_VALUES.each do |cap, values|
      values.each do |bad|
        assert_raises(ArgumentError, "#{cap} #{bad.inspect} through SandboxOptions.new must be rejected") do
          options(cap => bad)
        end
      end
    end
  end

  # @behavior RT-010
  def test_known_profiles_pass_through
    %i[permissive hermetic].each do |profile|
      assert_equal profile, options(profile: profile).profile,
                   "profile #{profile.inspect} through SandboxOptions.new must be readable back unchanged"
    end
  end

  # @behavior RT-019
  def test_gvl_modes_pass_through
    %i[hold release].each do |mode|
      assert_equal mode, options(gvl: mode).gvl,
                   "gvl mode #{mode.inspect} through SandboxOptions.new must be readable back unchanged"
    end
  end

  private

  def options(**given)
    Kobako::SandboxOptions.new(**GIVEN, **given)
  end
end
