# frozen_string_literal: true

require "test_helper"

# Unit tests for Kobako::Catalog::Snippet::Table — the per-Sandbox
# insertion-ordered registry of preloaded snippets (docs/behavior.md
# B-32 / E-33 / E-34). Behavioural coverage at the Sandbox#preload
# boundary lives in test/test_sandbox_preload.rb; this file pins the
# table's own contract.
class TestCatalogSnippetTableRegistration < Minitest::Test
  def setup
    @table = Kobako::Catalog::Snippet::Table.new
  end

  def test_new_table_is_empty
    assert @table.empty?
    assert_equal 0, @table.size
    assert_equal [], @table.names
  end

  def test_register_stores_under_symbol_name
    name = @table.register(code: "X = 1", name: :Helper)

    assert_equal :Helper, name
    assert_equal 1, @table.size
    assert_includes @table.names, :Helper
  end

  def test_register_accepts_string_name_and_normalizes_to_symbol
    @table.register(code: "Y = 2", name: "Worker")

    assert_equal [:Worker], @table.names
  end

  def test_register_preserves_insertion_order
    @table.register(code: "A", name: :Alpha)
    @table.register(code: "B", name: :Beta)
    @table.register(code: "C", name: :Gamma)

    assert_equal %i[Alpha Beta Gamma], @table.names
  end

  def test_each_yields_source_entries_in_insertion_order
    @table.register(code: "A", name: :Alpha)
    @table.register(code: "B", name: :Beta)

    entries = @table.each.to_a

    assert_equal :Alpha, entries.first.name
    assert_equal "A", entries.first.body
    assert_equal :Beta, entries.last.name
    assert_equal "B", entries.last.body
  end

  # E-34
  def test_register_rejects_name_not_matching_constant_pattern
    %i[lowercase _Leading 1Digit].each do |bad|
      err = assert_raises(ArgumentError) { @table.register(code: "X", name: bad) }
      assert_match(/snippet name must match/, err.message)
    end
  end

  def test_register_rejects_name_of_wrong_type
    err = assert_raises(ArgumentError) { @table.register(code: "X", name: 42) }
    assert_match(/must be a Symbol or String/, err.message)
  end

  def test_register_rejects_non_string_code
    err = assert_raises(ArgumentError) { @table.register(code: nil, name: :Helper) }
    assert_match(/code must be a String/, err.message)
  end

  def test_register_rejects_non_string_binary
    err = assert_raises(ArgumentError) { @table.register(binary: 42) }
    assert_match(/binary must be a String/, err.message)
  end

  def test_register_rejects_no_keyword_call
    err = assert_raises(ArgumentError) { @table.register }
    assert_match(/missing keyword/, err.message)
  end

  def test_register_rejects_combining_binary_with_code
    err = assert_raises(ArgumentError) { @table.register(code: "X = 1", binary: "RITE") }
    assert_match(%r{cannot combine binary: with code: / name:}, err.message)
  end

  def test_register_rejects_combining_binary_with_name
    err = assert_raises(ArgumentError) { @table.register(binary: "RITE", name: :Helper) }
    assert_match(%r{cannot combine binary: with code: / name:}, err.message)
  end

  # E-33
  def test_register_rejects_duplicate_name
    @table.register(code: "first body", name: :Worker)
    err = assert_raises(ArgumentError) { @table.register(code: "second body", name: :Worker) }
    assert_match(/already preloaded/, err.message)
  end

  def test_register_re_encodes_body_as_utf8
    bytes = String.new("X = 1", encoding: Encoding::ASCII_8BIT)
    @table.register(code: bytes, name: :Helper)
    body = @table.each.to_a.first.body

    assert_equal Encoding::UTF_8, body.encoding
    assert_equal "X = 1", body
  end

  def test_register_detaches_body_from_caller_reference
    original = +"X = 1"
    @table.register(code: original, name: :Helper)
    original << " # mutated"

    body = @table.each.to_a.first.body
    assert_equal "X = 1", body
  end
end

# docs/wire-codec.md § Invocation channels: Frame 3 is a msgpack array of
# per-entry maps. Source entries carry "name" / "kind" = "source" /
# "body" (UTF-8 str); Binary entries carry "kind" = "bytecode" / "body"
# (bin) and no "name". The encoder lives on Table to keep wire knowledge
# in one place (mirroring Kobako::Transport.encode_request /
# encode_response on entry-tier carriers).
class TestCatalogSnippetTableEncoding < Minitest::Test
  def setup
    @table = Kobako::Catalog::Snippet::Table.new
  end

  def test_encode_empty_table_serializes_to_empty_msgpack_array
    decoded = MessagePack.unpack(@table.encode)

    assert_equal [], decoded
  end

  def test_encode_source_entry_wire_shape
    @table.register(code: "X = 1", name: :Helper)

    decoded = MessagePack.unpack(@table.encode)

    assert_equal 1, decoded.length
    assert_equal({ "name" => "Helper", "kind" => "source", "body" => "X = 1" }, decoded.first)
  end

  def test_encode_binary_entry_omits_name_and_carries_bin_body
    @table.register(binary: "RITE\x00bytes")

    decoded = MessagePack.unpack(@table.encode)

    assert_equal 1, decoded.length
    assert_equal({ "kind" => "bytecode", "body" => "RITE\x00bytes".b }, decoded.first)
    refute_includes decoded.first.keys, "name",
                    "binary entry's canonical name lives in bytecode debug_info, not on the wire"
  end

  def test_encode_preserves_insertion_order_across_mixed_entry_kinds
    @table.register(code: "A", name: :Alpha)
    @table.register(binary: "RITE\x00first")
    @table.register(code: "B", name: :Beta)

    decoded = MessagePack.unpack(@table.encode)

    assert_equal(%w[source bytecode source], decoded.map { |e| e["kind"] })
    assert_equal "Alpha", decoded[0]["name"]
    assert_equal "Beta",  decoded[2]["name"]
  end
end
