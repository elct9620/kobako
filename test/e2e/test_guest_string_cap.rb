# frozen_string_literal: true

require "test_helper"

# E2E — the interpreter's own String length bound through real mruby. The
# guest's interpreter refuses a String of 1 MiB or more on its own, inside
# the guest where guest code can rescue it, well before any value nears the
# 16 MiB message cap the wire enforces. The memory budget is raised so the
# bound observed is the String's, not the budget's.
class TestE2EGuestStringCap < Minitest::Test
  include E2eGuestHelper

  ONE_MIB = 1 << 20
  BUILD_ONE_MIB = "begin; ('a' * 1_048_576).size; rescue ArgumentError; :refused; end"
  BUILD_UNDER_ONE_MIB = "('a' * 1_048_575).size"

  # @behavior MR-008
  def test_a_string_of_one_mib_raises_argument_error_inside_the_guest
    assert_equal :refused, roomy_sandbox.eval(BUILD_ONE_MIB).value,
                 "a 1 MiB String built through #eval must raise ArgumentError inside the guest, " \
                 "where guest code can rescue it, though the message cap is 16 MiB"
  end

  # @behavior MR-008
  def test_a_string_just_under_one_mib_is_built
    assert_equal ONE_MIB - 1, roomy_sandbox.eval(BUILD_UNDER_ONE_MIB).value,
                 "a String one byte under 1 MiB built through #eval must be built in full, so " \
                 "the bound sits at 1 MiB rather than below it"
  end

  private

  def roomy_sandbox
    Kobako::Sandbox.new(wasm_path: REAL_WASM, memory_limit: 64 * ONE_MIB)
  end
end
