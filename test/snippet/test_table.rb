# frozen_string_literal: true

require "test_helper"

# Unit tests for Kobako::Snippet::Table — the per-Sandbox insertion-
# ordered registry of preloaded snippets (docs/behavior.md B-32 / E-33 /
# E-34). Behavioural coverage at the Sandbox#preload boundary lives in
# test/test_sandbox_preload.rb; this file pins the table's own contract.
class TestSnippetTable < Minitest::Test
  def setup
    @table = Kobako::Snippet::Table.new
  end

  def test_new_table_is_empty
    assert @table.empty?
    assert_equal 0, @table.size
    assert_equal [], @table.names
  end

  def test_register_stores_under_symbol_name
    name = @table.register("X = 1", :Helper)

    assert_equal :Helper, name
    assert_equal 1, @table.size
    assert_includes @table.names, :Helper
  end

  def test_register_accepts_string_name_and_normalizes_to_symbol
    @table.register("Y = 2", "Worker")

    assert_equal [:Worker], @table.names
  end

  def test_register_preserves_insertion_order
    @table.register("A", :Alpha)
    @table.register("B", :Beta)
    @table.register("C", :Gamma)

    assert_equal %i[Alpha Beta Gamma], @table.names
  end

  def test_each_yields_source_entries_in_insertion_order
    @table.register("A", :Alpha)
    @table.register("B", :Beta)

    entries = @table.each.to_a

    assert_equal :Alpha, entries.first.name
    assert_equal "A", entries.first.body
    assert_equal :Beta, entries.last.name
    assert_equal "B", entries.last.body
  end

  # E-34
  def test_register_rejects_name_not_matching_constant_pattern
    %i[lowercase _Leading 1Digit].each do |bad|
      err = assert_raises(ArgumentError) { @table.register("X", bad) }
      assert_match(/snippet name must match/, err.message)
    end
  end

  def test_register_rejects_name_of_wrong_type
    err = assert_raises(ArgumentError) { @table.register("X", 42) }
    assert_match(/must be a Symbol or String/, err.message)
  end

  # E-33
  def test_register_rejects_duplicate_name
    @table.register("first body", :Worker)
    err = assert_raises(ArgumentError) { @table.register("second body", :Worker) }
    assert_match(/already preloaded/, err.message)
  end

  def test_register_re_encodes_body_as_utf8
    bytes = String.new("X = 1", encoding: Encoding::ASCII_8BIT)
    @table.register(bytes, :Helper)
    body = @table.each.to_a.first.body

    assert_equal Encoding::UTF_8, body.encoding
    assert_equal "X = 1", body
  end

  def test_register_detaches_body_from_caller_reference
    original = +"X = 1"
    @table.register(original, :Helper)
    original << " # mutated"

    body = @table.each.to_a.first.body
    assert_equal "X = 1", body
  end

  # docs/wire-codec.md § Invocation channels: Frame 3 is a msgpack array of
  # per-entry maps. Source entries carry "name" / "kind" = "source" /
  # "body" (UTF-8 str); Binary entries carry "kind" = "bytecode" / "body"
  # (bin) and no "name". The encoder lives on Table to keep wire
  # knowledge in one place (mirroring Kobako::RPC.encode_request /
  # encode_response on entry-tier carriers).
  def test_encode_empty_table_serializes_to_empty_msgpack_array
    decoded = MessagePack.unpack(@table.encode)

    assert_equal [], decoded
  end

  def test_encode_source_entry_wire_shape
    @table.register("X = 1", :Helper)

    decoded = MessagePack.unpack(@table.encode)

    assert_equal 1, decoded.length
    assert_equal({ "name" => "Helper", "kind" => "source", "body" => "X = 1" }, decoded.first)
  end

  def test_encode_binary_entry_omits_name_and_carries_bin_body
    @table.register_binary("RITE\x00bytes")

    decoded = MessagePack.unpack(@table.encode)

    assert_equal 1, decoded.length
    assert_equal({ "kind" => "bytecode", "body" => "RITE\x00bytes".b }, decoded.first)
    refute_includes decoded.first.keys, "name",
                    "binary entry's canonical name lives in bytecode debug_info, not on the wire"
  end

  def test_encode_preserves_insertion_order_across_mixed_entry_kinds
    @table.register("A", :Alpha)
    @table.register_binary("RITE\x00first")
    @table.register("B", :Beta)

    decoded = MessagePack.unpack(@table.encode)

    assert_equal(%w[source bytecode source], decoded.map { |e| e["kind"] })
    assert_equal "Alpha", decoded[0]["name"]
    assert_equal "Beta",  decoded[2]["name"]
  end
end
