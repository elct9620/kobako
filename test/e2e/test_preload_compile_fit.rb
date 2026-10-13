# frozen_string_literal: true

require "test_helper"

# E2E — a compile answer is kept only when the Preamble an invocation sends
# still fits once bytecode replaces the source, and that Preamble carries the
# bound paths too. The module's bytecode is about three times its 4 KiB of
# source; a 6 KiB bound path and a flag leaving 14 KiB make the bytecode fit
# alone and not beside the bindings.
class TestE2EPreloadCompileFit < Minitest::Test
  include E2eGuestHelper

  BIG = "module Big; #{(1..200).map { |i| "def self.m#{i} = #{i}" }.join("; ")}; end".freeze

  # @behavior S-199
  def test_bytecode_that_fits_only_without_the_bindings_leaves_the_source_replaying
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM, memory_limit: nil)
    sandbox.flag(:pad, "x" * ((16 << 20) - (14 << 10)), advisory: true)
    sandbox.bind("A#{"b" * (6 << 10)}")
    sandbox.preload(code: BIG, name: :Big)

    assert_equal [200, 200], Array.new(2) { sandbox.eval("Big.m200").value },
                 "preloaded source whose bytecode fits the Preamble only without its bound paths through " \
                 "#eval must keep replaying as the source"
  end
end
