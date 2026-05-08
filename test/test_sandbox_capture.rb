# frozen_string_literal: true

require "test_helper"

# Item #24 — WASI runtime stdout/stderr capture via wasmtime-wasi (SPEC.md §B-04).
#
# These tests verify end-to-end that:
#   1. Guest stdout writes surface in `sandbox.stdout` after `#run`.
#   2. Guest stderr is empty when the fixture only writes to stdout.
#   3. Buffers are per-run: a second #run does NOT see the first run's output.
#   4. Truncation still works: output beyond the cap gets `[truncated]` marker
#      from OutputBuffer without raising an error.
#
# The test-guest fixture now writes `"hello from test-guest\n"` to stdout
# via `println!` so these tests have something to assert against.
class TestSandboxCapture < Minitest::Test
  FIXTURE_PATH = File.expand_path("fixtures/test-guest.wasm", __dir__)

  def setup
    skip "native ext not compiled (run `bundle exec rake compile`)" unless defined?(Kobako::Wasm::Engine)
    skip "test-guest fixture missing (run `bundle exec rake fixtures:test_guest`)" \
      unless File.exist?(FIXTURE_PATH)
  end

  # B-04: stdout contains marker bytes written by the guest during `#run`.
  def test_stdout_buffer_contains_guest_output_after_run
    sandbox = Kobako::Sandbox.new(wasm_path: FIXTURE_PATH)
    sandbox.run("42")
    assert_includes sandbox.stdout, "hello from test-guest"
  end

  # B-04: stderr is empty when the guest only writes to stdout.
  def test_stderr_buffer_is_empty_when_guest_only_writes_stdout
    sandbox = Kobako::Sandbox.new(wasm_path: FIXTURE_PATH)
    sandbox.run("42")
    assert_equal "", sandbox.stderr
  end

  # B-03/B-04: a second #run resets the buffers — previous run output absent.
  def test_second_run_does_not_see_previous_run_stdout
    sandbox = Kobako::Sandbox.new(wasm_path: FIXTURE_PATH)
    sandbox.run("1")
    # The first run populates stdout; run again and the new buffer replaces it.
    sandbox.run("2")
    # After the second run we still see the marker (it's written every run),
    # but crucially we must NOT accumulate two copies of it.
    marker_count = sandbox.stdout.scan("hello from test-guest").length
    assert_equal 1, marker_count,
                 "stdout should contain exactly one copy of the marker (per-run reset)"
  end

  # B-04: truncation via OutputBuffer is preserved under WASI capture path.
  # Use a very small cap so a single println! overflows it.
  def test_stdout_truncation_marker_when_output_exceeds_cap
    tiny_cap = 5 # "hello from test-guest\n" is >> 5 bytes
    sandbox = Kobako::Sandbox.new(wasm_path: FIXTURE_PATH, stdout_limit: tiny_cap)
    sandbox.run("42")
    assert_includes sandbox.stdout, "[truncated]",
                    "stdout must contain the [truncated] marker when cap is exceeded"
  end
end
