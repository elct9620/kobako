# frozen_string_literal: true

require "test_helper"

# E2E — every way an artifact can fail to become a Sandbox's runtime is a
# construction failure: one that cannot be read, one the engine cannot
# link, one it cannot instantiate. None of them reaches an invocation, so
# none is attributed as one of its outcomes.
class TestRuntimeConstruction < Minitest::Test
  include GuestGuard

  def setup
    require_native_ext!
  end

  # A directory stands where the artifact should be: the path exists, so
  # this is not the absent-artifact case, but nothing can read it as one.
  # Each pairs the artifact with the step its message names, so a case
  # failing earlier than the step it stands for does not pass for it.
  UNCONSTRUCTABLE = {
    "an artifact that cannot be read" => [TestPaths.fixture(""), /failed to read/],
    "an artifact the engine cannot link" => [TestPaths.fixture("minimal_unlinkable.wat"), /unknown import/],
    "an artifact the engine cannot instantiate" => [TestPaths.fixture("minimal_uninstantiable.wat"),
                                                    /error while executing/]
  }.freeze

  # @behavior RT-063
  def test_an_artifact_that_cannot_become_a_runtime_is_a_construction_failure
    UNCONSTRUCTABLE.each do |artifact, (path, step)|
      err = assert_raises(Kobako::SetupError, "#{artifact} through Sandbox.new must fail construction") do
        Kobako::Sandbox.new(wasm_path: path)
      end
      assert_match step, err.message, "#{artifact} must fail at the step it stands for"
    end
  end

  # The unlinkable module asks the host for one name beyond the dispatch
  # entry; the host answering it at either posture would mean the guest
  # can reach something besides WASI and the dispatch entry.
  # @behavior RT-064
  def test_the_host_links_nothing_beyond_the_dispatch_entry_at_either_posture
    Kobako::SandboxOptions::PROFILES.each do |profile|
      err = assert_raises(Kobako::SetupError) do
        Kobako::Sandbox.new(wasm_path: TestPaths.fixture("minimal_unlinkable.wat"), profile: profile)
      end
      assert_match(/unknown import/, err.message,
                   "a Sandbox built #{profile} must link no host import beyond the dispatch entry")
    end
  end
end
