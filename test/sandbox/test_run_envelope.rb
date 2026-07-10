# frozen_string_literal: true

require "test_helper"

# The #run invocation-envelope reservation (docs/behavior/errors.md
# E-31): the guest's `__kobako_alloc` reporting exhaustion (returns 0)
# is a runtime-intact host-side failure — it surfaces as
# Kobako::SandboxError, never as a trap, and never reaches the guest
# entry point. Driven by a frozen wat fixture whose allocator always
# reports exhaustion.
class TestSandboxRunEnvelope < Minitest::Test
  ALLOC_ZERO_FIXTURE_PATH = File.expand_path("../fixtures/minimal_alloc_zero.wat", __dir__)

  def setup
    skip "native ext not compiled (run `bundle exec rake compile`)" unless defined?(Kobako::Runtime)
    skip "minimal_alloc_zero.wat fixture missing" unless File.exist?(ALLOC_ZERO_FIXTURE_PATH)
  end

  def test_run_raises_sandbox_error_when_guest_alloc_reports_exhaustion
    sandbox = Kobako::Sandbox.new(wasm_path: ALLOC_ZERO_FIXTURE_PATH)

    err = assert_raises(Kobako::SandboxError) { sandbox.run(:Worker) }
    assert_match(/could not allocate input buffer/, err.message,
                 "an exhausted guest allocator during #run envelope reservation must " \
                 "surface as a runtime-intact SandboxError")
  end
end
