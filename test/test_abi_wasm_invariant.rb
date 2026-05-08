# frozen_string_literal: true

# Real-tier E2E test for the wire ABI invariant (SPEC item #9 / item #26
# build-pipeline guard preview).
#
# Builds the kobako-wasm crate with `cargo build --target wasm32-wasip1
# --release`, then parses the resulting .wasm and asserts:
#
#   * Exactly 1 host import: `env.__kobako_rpc_call` typed `(i32 i32) -> i64`
#   * Exactly 3 guest exports with kobako names — `__kobako_run`,
#     `__kobako_alloc`, `__kobako_take_outcome` — and their SPEC signatures
#   * No additional kobako-namespaced imports or exports leaked
#
# The parser is a hand-written ~80-line walker over the wasm binary's import,
# export, type, and function sections (format: https://webassembly.github.io/spec/core/binary/modules.html).
# We inspect the wasm directly to avoid a dependency on wasm-tools / wasmparser.
#
# Standard `wasi_snapshot_preview1` imports (clock_time_get, fd_write, etc.)
# emitted by wasi-libc are tolerated — only the kobako protocol surface is
# constrained. Standard wasm exports `memory` and `_initialize` are likewise
# tolerated; the SPEC invariant constrains kobako-named exports.
#
# Gated behind KOBAKO_E2E_BUILD=1 because a wasm32-wasip1 build requires a
# Rust target install and is slow.

require "minitest/autorun"
require "open3"

$LOAD_PATH.unshift File.expand_path("../lib", __dir__)
require "kobako/abi"

class TestAbiWasmInvariant < Minitest::Test
  PROJECT_ROOT = File.expand_path("..", __dir__)
  CRATE_DIR    = File.join(PROJECT_ROOT, "wasm", "kobako-wasm")
  CARGO_TOML   = File.join(CRATE_DIR, "Cargo.toml")

  def test_kobako_abi_invariant_in_compiled_wasm
    skip "set KOBAKO_E2E_BUILD=1 to enable the wasm32-wasip1 invariant build" unless ENV["KOBAKO_E2E_BUILD"] == "1"
    skip_unless_cargo

    out, status = Open3.capture2e(
      "cargo", "build", "--manifest-path", CARGO_TOML,
      "--target", "wasm32-wasip1", "--release"
    )
    assert status.success?, "cargo build --target wasm32-wasip1 failed:\n#{out}"

    wasm_path = File.join(CRATE_DIR, "target", "wasm32-wasip1", "release", "kobako_wasm.wasm")
    assert File.file?(wasm_path), "expected wasm artifact at #{wasm_path}"

    parsed = parse_wasm_sections(File.binread(wasm_path))

    # ----- Imports -----
    kobako_imports = parsed[:imports].select { |i| i[:module] == Kobako::ABI::IMPORT_MODULE && i[:name].start_with?("__kobako_") }
    assert_equal 1, kobako_imports.size,
                 "SPEC pins exactly 1 host import; saw: #{kobako_imports.inspect}"

    rpc_import = kobako_imports.first
    assert_equal Kobako::ABI::IMPORT_NAME, rpc_import[:name]
    assert_equal :func, rpc_import[:kind]
    sig = parsed[:types][rpc_import[:type_idx]]
    assert_equal({ params: %i[i32 i32], results: [:i64] }, sig,
                 "__kobako_rpc_call signature must be (i32 i32) -> i64 per SPEC")

    # ----- Exports -----
    kobako_exports = parsed[:exports].select { |e| e[:name].start_with?("__kobako_") }
    exported_names = kobako_exports.map { |e| e[:name] }.sort
    assert_equal Kobako::ABI::EXPORT_NAMES.sort, exported_names,
                 "SPEC pins exactly 3 guest exports (kobako-namespaced); saw: #{exported_names.inspect}"

    expected_sigs = {
      "__kobako_run" => { params: [], results: [] },
      "__kobako_alloc" => { params: [:i32], results: [:i32] },
      "__kobako_take_outcome" => { params: [], results: [:i64] }
    }

    kobako_exports.each do |exp|
      assert_equal :func, exp[:kind], "#{exp[:name]} must be a func export"
      func_sig = parsed[:func_sig].call(exp[:index])
      assert_equal expected_sigs[exp[:name]], func_sig,
                   "#{exp[:name]} signature mismatch; expected #{expected_sigs[exp[:name]].inspect}, got #{func_sig.inspect}"
    end
  end

  private

  def skip_unless_cargo
    return if system("which cargo > /dev/null 2>&1")

    skip "cargo not installed; install Rust toolchain to exercise the wasm crate"
  end

  # ---------------------------------------------------------------------------
  # Minimal wasm binary parser — covers types (1), imports (2), functions (3),
  # and exports (7). Just enough to verify the ABI invariant.
  # See: https://webassembly.github.io/spec/core/binary/modules.html
  # ---------------------------------------------------------------------------

  VAL_TYPES = { 0x7f => :i32, 0x7e => :i64, 0x7d => :f32, 0x7c => :f64 }.freeze

  def parse_wasm_sections(bytes)
    raise "not a wasm binary" unless bytes[0, 4] == "\x00asm".b
    raise "unsupported wasm version" unless bytes[4, 4] == "\x01\x00\x00\x00".b

    pos = 8
    types = []
    imports = []
    func_type_indices = [] # index in func section -> type idx
    exports = []

    while pos < bytes.bytesize
      section_id = bytes.getbyte(pos)
      pos += 1
      cur = Cursor.new(bytes, pos)
      section_size = cur.uleb128!
      pos = cur.pos
      section_end = pos + section_size

      case section_id
      when 1  then types          = parse_type_section(bytes, pos)
      when 2  then imports        = parse_import_section(bytes, pos)
      when 3  then func_type_indices = parse_function_section(bytes, pos)
      when 7  then exports = parse_export_section(bytes, pos)
      end

      pos = section_end
    end

    # Number of imported funcs determines the index space offset for
    # locally-defined functions.
    imported_func_count = imports.count { |i| i[:kind] == :func }

    func_sig = lambda do |func_index|
      type_idx =
        if func_index < imported_func_count
          imp = imports.select { |i| i[:kind] == :func }[func_index]
          imp[:type_idx]
        else
          func_type_indices[func_index - imported_func_count]
        end
      types[type_idx]
    end

    { types: types, imports: imports, exports: exports, func_sig: func_sig }
  end

  # Cursor wrapper so nested helpers can advance a shared position.
  class Cursor
    attr_accessor :pos

    def initialize(bytes, pos)
      @bytes = bytes
      @pos = pos
    end

    def byte!
      b = @bytes.getbyte(@pos)
      @pos += 1
      b
    end

    def uleb128!
      result = 0
      shift = 0
      loop do
        b = byte!
        result |= (b & 0x7f) << shift
        break if b.nobits?(0x80)

        shift += 7
      end
      result
    end

    def string!
      len = uleb128!
      str = @bytes.byteslice(@pos, len).force_encoding(Encoding::UTF_8)
      @pos += len
      str
    end
  end

  def parse_type_section(bytes, pos)
    cur = Cursor.new(bytes, pos)
    count = cur.uleb128!
    Array.new(count) do
      raise "type form mismatch" unless cur.byte! == 0x60

      n_params = cur.uleb128!
      params = Array.new(n_params) { VAL_TYPES.fetch(cur.byte!) }
      n_results = cur.uleb128!
      results = Array.new(n_results) { VAL_TYPES.fetch(cur.byte!) }
      { params: params, results: results }
    end
  end

  def parse_import_section(bytes, pos)
    cur = Cursor.new(bytes, pos)
    count = cur.uleb128!
    Array.new(count) do
      mod_name = cur.string!
      field_name = cur.string!
      kind_byte = cur.byte!
      case kind_byte
      when 0x00 # func
        type_idx = cur.uleb128!
        { module: mod_name, name: field_name, kind: :func, type_idx: type_idx }
      when 0x01 # table — elemtype (1 byte) + limits (flag + 1 or 2 ulebs)
        cur.byte!
        flag = cur.byte!
        cur.uleb128!
        cur.uleb128! if flag == 1
        { module: mod_name, name: field_name, kind: :table }
      when 0x02 # memory — limits (flag + 1 or 2 ulebs)
        flag = cur.byte!
        cur.uleb128!
        cur.uleb128! if flag == 1
        { module: mod_name, name: field_name, kind: :memory }
      when 0x03 # global — valtype + mutability
        cur.byte!
        cur.byte!
        { module: mod_name, name: field_name, kind: :global }
      else
        raise "unknown import kind #{kind_byte}"
      end
    end
  end

  def parse_function_section(bytes, pos)
    cur = Cursor.new(bytes, pos)
    count = cur.uleb128!
    Array.new(count) { cur.uleb128! }
  end

  def parse_export_section(bytes, pos)
    cur = Cursor.new(bytes, pos)
    count = cur.uleb128!
    Array.new(count) do
      name = cur.string!
      kind_byte = cur.byte!
      idx = cur.uleb128!
      kind = { 0x00 => :func, 0x01 => :table, 0x02 => :memory, 0x03 => :global }.fetch(kind_byte)
      { name: name, kind: kind, index: idx }
    end
  end
end
