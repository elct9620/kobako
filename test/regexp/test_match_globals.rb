# frozen_string_literal: true

require "test_helper"

# Match backref globals parity (SPEC.md B-41). The curated regexp engine
# sets $~ / $1..$9 / $& / $` / $' after each successful match within an
# invocation. Expected values are the C-gem (Onigmo) oracle harvested from
# data/kobako.wasm.
class TestRegexpMatchGlobals < Minitest::Test
  include RegexpParityHelper

  def test_numbered_group_global
    assert_parity("12", '"a12" =~ /(\d+)/; $1',
                  "$1 holds the first capture after a successful match")
  end

  def test_match_data_global
    assert_parity("12", '"a12" =~ /(\d+)/; $~[0]',
                  "$~ holds the MatchData after a successful match")
  end

  def test_whole_match_global
    assert_parity("12", '"a12" =~ /\d+/; $&',
                  "$& holds the whole matched substring")
  end

  def test_pre_and_post_match_globals
    assert_parity(%w[xa y], '"xa12y" =~ /\d+/; [$`, $\']',
                  "$` and $' hold the text before and after the match")
  end
end
