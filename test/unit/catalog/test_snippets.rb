# frozen_string_literal: true

require "test_helper"

module Kobako
  # Unit tests for Kobako::Catalog::Snippets — the per-Sandbox
  # insertion-ordered registry of preloaded snippets (docs/behavior/invocation.md
  # B-32 / E-33 / E-34). Behavioural coverage at the Sandbox#preload
  # boundary lives in test/e2e/sandbox/test_preload.rb; this file pins the
  # table's own contract.
  #
  # The table exposes only #register (mutation) and #entries (the
  # invocation's projection) to the outside world — every observable
  # contract is therefore stated against #entries rather than internal
  # enumeration helpers.
  class CatalogSnippetsTest < Minitest::Test
    def setup
      @table = Catalog::Snippets.new
    end

    def test_a_new_table_has_no_entries
      assert_empty @table.entries,
                   "a table with nothing preloaded through #entries must be empty, never absent"
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

      body = @table.entries.first.last
      assert_equal Encoding::UTF_8, body.encoding
      assert_equal "X = 1", body
    end

    def test_register_detaches_body_from_caller_reference
      original = +"X = 1"
      @table.register(code: original, name: :Helper)
      original << " # mutated"

      assert_equal "X = 1", @table.entries.first.last
    end
  end

  # The projection every invocation frames into Frame 3
  # (docs/wire-codec.md § Invocation channels): one +[kind, name, body]+
  # triple per entry in insertion order. Source entries name themselves;
  # Binary entries carry no name, because a bytecode snippet's canonical
  # name lives in its RITE debug_info and is read by the guest at load
  # time. The projection lives on the collection so the leaf
  # Snippet::Source / Snippet::Binary entries stay pure carriers.
  class CatalogSnippetsEntriesTest < Minitest::Test
    def setup
      @table = Catalog::Snippets.new
    end

    def test_a_source_entry_carries_its_kind_name_and_body
      @table.register(code: "X = 1", name: :Helper)

      assert_equal [[:source, "Helper", "X = 1"]], @table.entries,
                   "a code: snippet through #entries must carry the source kind, its name, and its body"
    end

    def test_a_binary_entry_carries_no_name_and_keeps_its_bytes
      @table.register(binary: "RITE\x00bytes")

      assert_equal [[:bytecode, nil, "RITE\x00bytes".b]], @table.entries,
                   "a binary: snippet through #entries must carry the bytecode kind and no name"
    end

    # @behavior S-054
    def test_entries_preserve_insertion_order_across_mixed_kinds
      @table.register(code: "A", name: :Alpha)
      @table.register(binary: "RITE\x00first")
      @table.register(code: "B", name: :Beta)

      assert_equal [%i[source bytecode source], ["Alpha", nil, "Beta"]],
                   [@table.entries.map(&:first), @table.entries.map { |entry| entry[1] }],
                   "a mixed snippet table through #entries must stay in registration order"
    end

    def test_entries_after_register_include_the_newly_registered_entry
      @table.register(code: "A", name: :Alpha)
      first = @table.entries
      @table.register(code: "B", name: :Beta)

      assert_equal(["Alpha"], first.map { |entry| entry[1] })
      assert_equal %w[Alpha Beta], @table.entries.map { |entry| entry[1] },
                   "a snippet registered on an unsealed table must appear in the next #entries read"
    end
  end
end
