# frozen_string_literal: true

require "test_helper"

# What the match globals hold across the String methods that find every match
# at once. Each expected value is MRI's answer to the same guest code.
class TestRegexpStringGlobals < Minitest::Test
  include RegexpGuestHelper

  # @behavior RX-263
  def test_a_substitution_block_reads_a_capture_by_name
    assert_equal "fOO", eval_regexp('"foo".gsub(/(?<x>o)/) { $~[:x].upcase }'),
                 "a capture name through $~ in a String#gsub block must read that match's group"
    assert_equal 'f["x"]o', eval_regexp('"foo".sub(/(?<x>o)/) { $~.names.inspect }'),
                 "$~.names in a String#sub block must list the pattern's names"
  end

  # @behavior RX-263
  def test_a_substitution_block_reads_a_shared_name_by_the_group_that_took_part
    assert_equal "AB", eval_regexp('"ab".gsub(/(?<n>a)|(?<n>b)/) { $~[:n].upcase }'),
                 "a shared name through $~ in a String#gsub block must read the alternative each match took"
  end
end
