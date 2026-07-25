# frozen_string_literal: true

require "test_helper"

# The null-guest fixture completes an invocation without doing any guest
# work, which is what lets benchmark/host_invocation.rb read the host's
# per-invocation cost as a total rather than deriving it by subtracting a
# guest budget from one. These assertions are what keep the fixture
# honest: it must satisfy the whole invocation ABI, not merely load.
class TestSandboxNullGuest < Minitest::Test
  include GuestGuard

  FIXTURE_PATH = TestPaths.fixture("minimal_null_guest.wat")

  def setup
    require_fixture!(FIXTURE_PATH)
    @sandbox = Kobako::Sandbox.new(wasm_path: FIXTURE_PATH)
  end

  def test_an_eval_against_the_null_guest_completes_with_nil
    assert_nil @sandbox.eval("anything").value,
               "the null guest must answer every #eval with a nil Result, so a probe measures the " \
               "host's invocation path against a guest that contributes no work of its own"
  end

  def test_a_run_against_the_null_guest_completes_with_nil
    assert_nil @sandbox.run(:Anything, 42, name: :alice).value,
               "the null guest must answer #run too — the Run envelope and its arguments are encoded " \
               "host-side, so that path needs the same measurable floor as #eval"
  end

  def test_the_null_guest_reports_no_captured_output
    assert_equal "", @sandbox.eval("anything").stdout,
                 "a guest that writes nothing must leave the capture empty, so a probe's total " \
                 "carries the host's readout cost and no guest-produced bytes"
  end
end
