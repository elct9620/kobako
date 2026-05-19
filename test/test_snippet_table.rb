# frozen_string_literal: true

require "test_helper"

# Unit tests for Kobako::SnippetTable — the per-Sandbox ordered registry
# of preloaded source snippets (docs/behavior.md B-32 / E-33 / E-34).
# Behavioural coverage at the Sandbox#preload boundary lives in
# test_sandbox_preload.rb; this file pins the table's own contract.
class TestSnippetTable < Minitest::Test
  def setup
    @table = Kobako::SnippetTable.new
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

  def test_each_yields_pairs_in_insertion_order
    @table.register("A", :Alpha)
    @table.register("B", :Beta)

    pairs = @table.each.to_a

    assert_equal [%i[Alpha].first, "A"], pairs.first
    assert_equal [%i[Beta].first, "B"], pairs.last
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
    body = @table.each.to_a.first.last

    assert_equal Encoding::UTF_8, body.encoding
    assert_equal "X = 1", body
  end

  def test_register_detaches_body_from_caller_reference
    original = +"X = 1"
    @table.register(original, :Helper)
    original << " # mutated"

    body = @table.each.to_a.first.last
    assert_equal "X = 1", body
  end
end
