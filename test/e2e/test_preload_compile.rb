# frozen_string_literal: true

require "test_helper"

# E2E — preloaded source compiled by the bundled Guest Binary. A replay of
# the bytecode is meant to be indistinguishable from a replay of the source,
# so the source-form tests in test/e2e/test_preload.rb already run through
# it; this file pins what only the compile itself can show.
class TestE2EPreloadCompile < Minitest::Test
  include E2eGuestHelper
  include RuntimeOptions

  # Large enough that compiling it moves the memory peak of a replay.
  BIG = "module Big; #{(1..200).map { |i| "def self.m#{i} = #{i}" }.join("; ")}; end".freeze

  # Nothing a script can observe tells a bytecode replay from a source one,
  # so the table's projection is the witness that the guest answered.
  # @behavior S-188
  def test_the_bundled_guest_binary_compiles_preloaded_source
    table = compiled_table(Alpha: "module Alpha; X = 1; end", Beta: "module Beta; Y = Alpha::X + 1; end")

    assert_equal %i[bytecode bytecode], table.entries.map(&:first),
                 "preloaded source through Catalog::Snippets#compile on the bundled Guest Binary " \
                 "must come back as one bytecode per snippet"
  end

  # A source replay also compiles, so it takes more memory than a bytecode
  # replay of the same snippet. A Sandbox preloading source reports the peak
  # of one preloading that source's bytecode only when it replays bytecode.
  # @behavior S-186
  def test_a_sandbox_replays_its_preloaded_source_as_bytecode
    bytecode = compiled_table(Big: BIG).entries.first.last

    assert_equal peak_of { |sandbox| sandbox.preload(binary: bytecode) },
                 peak_of { |sandbox| sandbox.preload(code: BIG, name: :Big) },
                 "a Sandbox preloading source through #eval must replay it as bytecode, reporting the " \
                 "memory peak of a Sandbox preloading that bytecode"
  end

  # Compiling happens on an instance of its own before the first
  # invocation, so the first invocation reports what any later one does.
  # @behavior S-189
  def test_compiling_counts_against_no_invocation
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.preload(code: BIG, name: :Big)

    first, second = Array.new(2) { sandbox.eval("Big.m200").usage.memory_peak }

    assert_equal first, second,
                 "the first #eval of a Sandbox with preloaded source must report the same memory peak " \
                 "as a later one, since compiling that source is not part of any invocation"
  end

  # Compiled source replays as bytecode, where a blob mruby cannot read
  # answers ScriptError too; the snippet loaded, so its raise is its own.
  # @behavior S-190
  def test_preloaded_source_raising_script_error_keeps_its_class
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.preload(code: "raise ScriptError, 'config missing'", name: :Boom)

    err = assert_raises(Kobako::SandboxError) { sandbox.eval("1") }
    assert_equal "ScriptError", err.klass,
                 "preloaded source raising ScriptError itself through the first #eval must " \
                 "fail carrying ScriptError, as it did before it was compiled"
  end

  # @behavior S-199
  def test_bytecode_the_preamble_could_not_carry_leaves_the_source_replaying
    sandbox = source_bound_sandbox

    assert_equal [200, 200], Array.new(2) { sandbox.eval("Big.m200").value },
                 "preloaded source whose bytecode would not fit the Preamble through #eval must keep " \
                 "replaying as the source that does"
  end

  # Every other Sandbox replays its source as bytecode, so this is the one
  # place the source replay's top-level self is witnessed.
  # @behavior S-191
  def test_a_source_replay_shares_the_scripts_top_level_self
    sandbox = source_bound_sandbox
    sandbox.preload(code: "@answer = 42", name: :Answers)

    assert_equal 42, sandbox.eval("@answer").value,
                 "an instance variable a snippet replayed as source sets at its top level, read through " \
                 "#eval at the script's top level, must hold the value the snippet set"
  end

  private

  # A Sandbox preloading BIG whose Preamble can carry its source and not its
  # bytecode: BIG's bytecode is about three times its 4 KiB of source, and a
  # flag leaves 8 KiB of the Preamble's 16 MiB.
  def source_bound_sandbox
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM, memory_limit: nil)
    sandbox.flag(:pad, "x" * ((16 << 20) - (8 << 10)), advisory: true)
    sandbox.preload(code: BIG, name: :Big)
    sandbox
  end

  def compiled_table(sources)
    runtime = Kobako::Runtime.from_path(REAL_WASM, **runtime_options(timeout: 10))
    table = Kobako::Catalog::Snippets.new
    sources.each { |name, code| table.register(code: code, name: name) }
    table.compile { |entries| runtime.compile([], entries, []) }
    table
  end

  def peak_of
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    yield sandbox
    sandbox.eval("Big.m200").usage.memory_peak
  end
end
