# frozen_string_literal: true

require "test_helper"

module Kobako
  # Unit tests for Kobako::Catalog::Flags — the per-Sandbox table of flags
  # each invocation's Preamble carries. Behavioural coverage at the
  # Sandbox#flag boundary lives in test/e2e/sandbox/test_flag.rb.
  class CatalogFlagsTest < Minitest::Test
    def setup
      @table = Catalog::Flags.new
    end

    def test_a_flag_is_critical_unless_marked_advisory
      @table.register(:strict, "", advisory: false)
      @table.register("limit", "8", advisory: true)

      assert_equal [["strict", "", true], ["limit", "8", false]], @table.entries,
                   "flags through #register must project in order as name, value and criticality"
    end

    # @behavior S-185
    def test_a_name_or_value_of_the_wrong_kind_is_refused
      assert_raises(ArgumentError, "a flag name that is neither Symbol nor String must be refused") do
        @table.register(1, "", advisory: false)
      end
      assert_raises(ArgumentError, "a flag value that is not a String must be refused") do
        @table.register(:limit, 8, advisory: false)
      end
    end

    def test_a_value_read_as_bytes_is_carried_as_utf8
      @table.register(:mode, "on".b, advisory: false)

      assert_equal Encoding::UTF_8, @table.entries.first[1].encoding,
                   "a binary-tagged flag value through #register must be re-tagged as UTF-8"
    end

    # @behavior S-198
    def test_a_name_or_value_that_is_not_utf8_is_refused
      messages = [["\xff".b, ""], [:mode, "\xff".b]].map do |name, value|
        assert_raises(ArgumentError) { @table.register(name, value, advisory: true) }.message
      end

      assert_equal ["flag name must be UTF-8 text", "flag value must be UTF-8 text"], messages,
                   "a flag name or value whose bytes are not UTF-8 through #register must be refused, " \
                   "naming which one"
    end
  end
end
