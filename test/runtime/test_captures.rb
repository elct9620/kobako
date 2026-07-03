# frozen_string_literal: true

require "test_helper"

# Unit-level coverage of the per-invocation readouts +Kobako::Runtime+
# exposes beside the returned outcome bytes: the +#captures+ 4-tuple and
# the raw return-bytes value. Drives +Runtime+ directly (bypassing
# +Sandbox+) against the real +data/kobako.wasm+ so the contract being
# pinned is "what the ext hands back", not the Sandbox-side decomposition.
#
# Sandbox-level consumption of the same readouts is covered through
# +test/sandbox/+ and the +test/e2e/+ journeys (including the B-04
# captures-on-trap cases in +test/e2e/test_caps.rb+); this file
# deliberately stays at the Runtime seam so a regression in the magnus
# binding surfaces here, not via indirect Sandbox assertions.
class TestRuntimeCaptures < Minitest::Test
  KOBAKO_WASM = File.expand_path("../../data/kobako.wasm", __dir__)

  def setup
    skip "native ext not compiled (run `bundle exec rake compile`)" unless defined?(Kobako::Runtime)
    skip "guest wasm not built (run `bundle exec rake wasm:build`)" unless File.exist?(KOBAKO_WASM)
  end

  # The ext encodes the invocation outcome and the capture slots into
  # specific Ruby shapes (binary String for the byte fields, bool for the
  # truncation flags) — pin them so a magnus binding change cannot
  # silently shift a type past RBS, which does not verify what a C
  # extension actually returns.
  def test_eval_returns_bytes_and_captures_with_documented_raw_types
    runtime = drive_eval("42")
    stdout_bytes, stdout_truncated, stderr_bytes, stderr_truncated = runtime.captures

    assert_kind_of String, @return_bytes
    assert_kind_of String, stdout_bytes
    assert_kind_of String, stderr_bytes
    assert_includes [true, false], stdout_truncated
    assert_includes [true, false], stderr_truncated
  end

  # The 4-tuple layout is positional; a reader reorder in the ext would
  # silently swap the channels. Writing distinct content to each channel
  # in one run pins stdout to slot 0 and stderr to slot 2.
  def test_captures_tuple_keeps_stdout_and_stderr_slots_apart
    runtime = drive_eval('$stdout.puts "to-out"; $stderr.puts "to-err"; 1')
    stdout_bytes, _stdout_truncated, stderr_bytes, _stderr_truncated = runtime.captures

    assert_equal "to-out\n", stdout_bytes,
                 "captures slot 0 must carry the stdout channel"
    assert_equal "to-err\n", stderr_bytes,
                 "captures slot 2 must carry the stderr channel"
  end

  # Before any invocation the readout is the pre-invocation sentinel —
  # empty bytes, flags down — so a fresh Runtime never leaks a previous
  # process state or nil into the Sandbox's Capture wrapping.
  def test_captures_before_any_invocation_returns_empty_sentinel
    runtime = Kobako::Runtime.from_path(KOBAKO_WASM, nil, nil, nil, nil, :hermetic)

    assert_equal ["", false, "", false], runtime.captures
  end

  private

  # Minimal Runtime driver that mirrors +Sandbox#eval+'s wiring without
  # the Sandbox wrapper. Builds an empty Catalog::Namespaces / Snippet table
  # so the encoded preamble + encoded snippets are both wire-valid, registers
  # a guard Proc on +on_dispatch=+ (no Service callbacks expected from
  # the simple eval sources used by these tests), stashes the returned
  # outcome bytes in +@return_bytes+, and returns the Runtime so callers
  # can read +#captures+ afterwards.
  def drive_eval(code)
    handler = Kobako::Catalog::Handles.new
    services = Kobako::Catalog::Namespaces.new(handler: handler)
    snippets = Kobako::Catalog::Snippets.new

    runtime = Kobako::Runtime.from_path(KOBAKO_WASM, nil, nil, nil, nil, :hermetic)
    runtime.on_dispatch = ->(_, _) { raise "unexpected dispatch in eval-only captures test" }

    @return_bytes = runtime.eval(services.encode, code.b, snippets.encode)
    runtime
  end
end
