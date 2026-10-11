# frozen_string_literal: true

require "test_helper"

# E2E — preloaded source compiled by the bundled Guest Binary. A replay of
# the bytecode is meant to be indistinguishable from a replay of the source,
# so the source-form tests in test/e2e/test_preload.rb already run through
# it; this file pins what only the compile itself can show.
class TestE2EPreloadCompile < Minitest::Test
  include E2eGuestHelper

  # Nothing a script can observe tells a bytecode replay from a source one,
  # so the table's projection is the witness that the guest answered.
  # @behavior S-188
  def test_the_bundled_guest_binary_compiles_preloaded_source
    runtime = Kobako::Runtime.from_path(REAL_WASM, 10, nil, nil, nil, :hermetic, :hold)
    table = Kobako::Catalog::Snippets.new
    table.register(code: "module Alpha; X = 1; end", name: :Alpha)
    table.register(code: "module Beta; Y = Alpha::X + 1; end", name: :Beta)

    table.compile(runtime, [])

    assert_equal %i[bytecode bytecode], table.entries.map(&:first),
                 "preloaded source through Catalog::Snippets#compile on the bundled Guest Binary " \
                 "must come back as one bytecode per snippet"
  end

  # Compiling happens on an instance of its own before the first
  # invocation, so the first invocation reports what any later one does.
  # @behavior S-189
  def test_compiling_counts_against_no_invocation
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.preload(code: "module Big; #{(1..200).map { |i| "def self.m#{i} = #{i}" }.join("; ")}; end",
                    name: :Big)

    first, second = Array.new(2) { sandbox.eval("Big.m200").usage.memory_peak }

    assert_equal first, second,
                 "the first #eval of a Sandbox with preloaded source must report the same memory peak " \
                 "as a later one, since compiling that source is not part of any invocation"
  end
end
