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

  # @behavior RX-264
  def test_a_scan_block_reads_the_match_globals_of_each_match
    assert_equal %w[o o], eval_regexp('r = []; "foo".scan(/(o)/) { r << $1 }; r'),
                 "$1 in a String#scan block must hold that iteration's capture"
    assert_equal [1, 2], eval_regexp('r = []; "foo".scan(/o/) { r << $~.begin(0) }; r'),
                 "$~ in a String#scan block must be that iteration's match"
    assert_equal %w[o o], eval_regexp('r = []; "foo".scan(/(?<x>o)/) { r << $~[:x] }; r'),
                 "a capture name through $~ in a String#scan block must read that iteration's group"
  end

  # @behavior RX-265
  def test_scanning_and_global_substitution_leave_the_last_match
    assert_equal 2, eval_regexp('"foo".scan(/o/); $~.begin(0)'),
                 "$~ after String#scan must be the last match"
    assert_equal 2, eval_regexp('"foo".gsub(/o/, "y"); $~.begin(0)'),
                 "$~ after String#gsub with a replacement must be the last match"
    assert_equal "o", eval_regexp('"foo".gsub(/(?<x>o)/, "y"); $~[:x]'),
                 "$~ after String#gsub must still read captures by name"
    assert_equal "o", eval_regexp('"foo".gsub("o", "y"); $~[0]'),
                 "$~ after String#gsub with a String pattern must be the last match"
  end

  # @behavior RX-266
  def test_single_substitution_leaves_its_match
    assert_equal 1, eval_regexp('"foo".sub(/o/, "y"); $~.begin(0)'),
                 "$~ after String#sub must be the match it replaced"
    assert_equal "o", eval_regexp('s = "foo"; s.sub!(/o/, "y"); $~[0]'),
                 "$~ after String#sub! must be the match it replaced"
  end

  # @behavior RX-267
  def test_a_method_that_found_nothing_clears_the_last_match
    %w[scan(/z/) gsub(/z/,"y") sub(/z/,"y")].each do |call|
      assert_nil eval_regexp(%("x" =~ /x/; "foo".#{call}; $~)),
                 "String##{call} finding nothing must clear $~"
    end
  end

  # gsub and scan end by settling on their own last match; sub sets its match
  # once before its block, so a match inside that block stays.
  # @behavior RX-268
  def test_a_match_inside_a_block_stays_only_after_a_single_substitution
    assert_equal "o", eval_regexp('"foo".gsub(/o/) { "x" =~ /x/; "y" }; $~[0]'),
                 "a match inside a String#gsub block must give way to gsub's last match afterwards"
    assert_equal "o", eval_regexp('"foo".scan(/o/) { "x" =~ /x/ }; $~[0]'),
                 "a match inside a String#scan block must give way to scan's last match afterwards"
    assert_equal "x", eval_regexp('"foo".sub(/o/) { "x" =~ /x/; "y" }; $~[0]'),
                 "a match inside a String#sub block must remain as $~ afterwards"
  end

  # Splitting finds matches too, yet MRI leaves the match globals alone.
  # @behavior RX-269
  def test_splitting_leaves_the_last_match_alone
    assert_equal "x", eval_regexp('"x" =~ /x/; "a b".split(/ /); $~[0]'),
                 "String#split must leave $~ as it was"
  end
end
