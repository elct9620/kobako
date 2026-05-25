# frozen_string_literal: true

require "test_helper"

# Unit tests for Kobako::Catalog::Snippets — the per-Sandbox
# insertion-ordered registry of preloaded snippets (docs/behavior.md
# B-32 / E-33 / E-34). Behavioural coverage at the Sandbox#preload
# boundary lives in test/test_sandbox_preload.rb; this file pins the
# table's own contract.
#
# The table exposes only #register (mutation) and #encode (wire shape)
# to the outside world — every observable contract is therefore stated
# against the msgpack-decoded #encode output rather than internal
# enumeration helpers.
class TestCatalogSnippetTableRegistration < Minitest::Test
  def setup
    @table = Kobako::Catalog::Snippets.new
  end

  def test_new_table_encodes_to_empty_msgpack_array
    assert_equal [], decoded
  end

  def test_register_returns_symbol_name_for_source_form
    assert_equal :Helper, @table.register(code: "X = 1", name: :Helper)
  end

  def test_register_returns_nil_for_binary_form
    assert_nil @table.register(binary: "RITE")
  end

  def test_register_accepts_string_name_and_normalizes_to_symbol
    assert_equal :Worker, @table.register(code: "Y = 2", name: "Worker")
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

    body = decoded.first["body"]
    assert_equal Encoding::UTF_8, body.encoding
    assert_equal "X = 1", body
  end

  def test_register_detaches_body_from_caller_reference
    original = +"X = 1"
    @table.register(code: original, name: :Helper)
    original << " # mutated"

    assert_equal "X = 1", decoded.first["body"]
  end

  private

  def decoded
    MessagePack.unpack(@table.encode)
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
    @table = Kobako::Catalog::Snippets.new
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

  def test_encode_preserves_insertion_order_across_source_entries
    @table.register(code: "A", name: :Alpha)
    @table.register(code: "B", name: :Beta)
    @table.register(code: "C", name: :Gamma)

    decoded = MessagePack.unpack(@table.encode)

    assert_equal(%w[Alpha Beta Gamma], decoded.map { |e| e["name"] })
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
