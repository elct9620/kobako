# frozen_string_literal: true

require "test_helper"

# Block form of Regexp#match / String#match (SPEC.md B-41). A successful
# match yields its MatchData to the block and returns the block's result; a
# miss returns nil without calling the block.
class TestRegexpMatchBlock < Minitest::Test
  include RegexpGuestHelper

  def test_regexp_match_yields_matchdata_to_block
    assert_equal "12", eval_regexp('/(\d+)/.match("a12"){|m| m[0] }'),
                 "Regexp#match yields the MatchData to the block and returns the block result"
  end

  def test_regexp_match_skips_block_on_miss
    assert_nil eval_regexp('/x/.match("abc"){|m| "called" }'),
               "Regexp#match returns nil without calling the block when nothing matches"
  end

  def test_string_match_yields_matchdata_to_block
    assert_equal "12", eval_regexp('"a12".match(/(\d+)/){|m| m[1] }'),
                 "String#match forwards a block through to Regexp#match"
  end
end
