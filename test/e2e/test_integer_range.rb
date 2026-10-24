# frozen_string_literal: true

require "test_helper"

# E2E — integer representability across the wire through real mruby
# (docs/wire/payload-msgpack.md § Integer Range). msgpack's integer
# families reach from -2**63 to 2**64-1, every one of which the guest holds,
# so the only bound is the one a guest Integer meets on the way out.
class TestE2EIntegerRange < Minitest::Test
  include E2eGuestHelper

  U64_MAX = 18_446_744_073_709_551_615

  # @behavior CD-002
  # The value sits one past what a signed 64-bit read holds, so it reaches
  # the script only as a big integer and leaves only as an unsigned one.
  def test_widest_wire_integer_crosses_both_ways
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("Clock::Max", -> { U64_MAX })

    assert_equal [U64_MAX, "18446744073709551615"],
                 sandbox.eval("x = Clock::Max.call; [x, x.to_s]").value,
                 "2**64-1 answered by a Service through Sandbox#eval must reach the script and " \
                 "come back as that exact Integer"
  end

  # @behavior CD-053
  # Each side of the wire's range is paired against the values just inside
  # it, which cross.
  def test_guest_integer_past_the_wire_range_does_not_cross_to_the_host
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)

    %w[2**64 -(2**63)-1].each do |source|
      err = assert_raises(Kobako::SandboxError) { sandbox.eval(source) }

      assert_equal "return value of type Integer is not a supported sandbox value type", err.message,
                   "#{source} answered through Sandbox#eval must fail the invocation as a value " \
                   "the wire does not carry"
    end
  end

  # @behavior CD-054
  # mruby normalizes a big integer only when its magnitude fits the
  # positive range, so -2**63 computed this way stays big inside the guest.
  def test_smallest_i64_computed_from_a_big_integer_crosses
    assert_equal(-9_223_372_036_854_775_808,
                 Kobako::Sandbox.new(wasm_path: REAL_WASM).eval("-(2**63)").value,
                 "-(2**63) answered through Sandbox#eval must reach the host as -9223372036854775808")
  end

  # @behavior MR-026
  # The parser hands a literal past 32 bits to mruby-bigint, which must keep
  # every digit rather than raise or round.
  def test_integer_literal_past_i64_range_stays_exact_inside_the_guest
    digits = Kobako::Sandbox.new(wasm_path: REAL_WASM).eval("18446744073709551617.to_s").value

    assert_equal "18446744073709551617", digits,
                 "the literal 18446744073709551617 through Sandbox#eval must read back with every digit"
  end
end
