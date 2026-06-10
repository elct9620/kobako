# frozen_string_literal: true

require "test_helper"

# Match backref globals contract (SPEC.md B-41). kobako-regexp sets
# $~ / $1..$9 / $& / $` / $' after each successful match within an
# invocation, and refreshes them on every iteration of a gsub block.
class TestRegexpMatchGlobals < Minitest::Test
  include RegexpGuestHelper

  def test_numbered_group_global
    assert_equal "12", eval_regexp('"a12" =~ /(\d+)/; $1'),
                 "$1 holds the first capture after a successful match"
  end

  def test_match_data_global
    assert_equal "12", eval_regexp('"a12" =~ /(\d+)/; $~[0]'),
                 "$~ holds the MatchData after a successful match"
  end

  def test_whole_match_global
    assert_equal "12", eval_regexp('"a12" =~ /\d+/; $&'),
                 "$& holds the whole matched substring"
  end

  def test_pre_and_post_match_globals
    assert_equal %w[xa y], eval_regexp('"xa12y" =~ /\d+/; [$`, $\']'),
                 "$` and $' hold the text before and after the match"
  end

  # $+ holds the last capture group that participated (MRI semantics): the
  # highest-numbered non-nil group, or nil when the pattern has no groups.
  def test_last_group_global_is_highest_capture
    assert_equal "b", eval_regexp('"a1b" =~ /(\d)(\w)/; $+'),
                 "$+ holds the last capture group that matched"
  end

  def test_last_group_global_is_nil_without_groups
    assert_nil eval_regexp('"abc" =~ /b/; $+'),
               "$+ is nil when the pattern has no capture groups"
  end

  # $1 inside a gsub block refreshes to each iteration's capture rather than
  # staying pinned to the first match.
  def test_dollar1_refreshes_per_gsub_iteration
    assert_equal "a1!b2!", eval_regexp('"a1b2".gsub(/(\d)/){ $1 + "!" }'),
                 "$1 inside a gsub block refreshes to each iteration's capture"
  end

  # Regexp.last_match tracks $~ in lock-step (MRI keeps them equal); the
  # writer lets a caller save and restore the match around an inner match.
  def test_last_match_returns_the_most_recent_match
    assert_equal "12", eval_regexp('"ab12" =~ /\d+/; Regexp.last_match[0]'),
                 "Regexp.last_match returns the MatchData of the most recent match"
  end

  def test_last_match_is_nil_before_any_match
    assert_nil eval_regexp("Regexp.last_match"),
               "Regexp.last_match is nil before any match has run"
  end

  def test_last_match_assignment_round_trips_saved_match
    assert_equal "x",
                 eval_regexp('"x" =~ /x/; m = Regexp.last_match; "yy" =~ /y/; ' \
                             "Regexp.last_match = m; Regexp.last_match[0]"),
                 "Regexp.last_match= restores a previously saved match"
  end

  # The numbered globals are views of $~, so a saved-then-restored match must
  # carry $1 with it — not leave it pinned to the intervening match.
  def test_last_match_assignment_refreshes_numbered_globals
    assert_equal "x",
                 eval_regexp('"x" =~ /(x)/; m = Regexp.last_match; "yy" =~ /(y)/; ' \
                             "Regexp.last_match = m; $1"),
                 "$1 follows Regexp.last_match= so the numbered globals stay views of $~"
  end

  # Clearing $~ to nil clears its views too; $1 must not survive the reset.
  def test_last_match_assignment_to_nil_clears_numbered_globals
    assert_nil eval_regexp('"a1" =~ /(\d)/; Regexp.last_match = nil; $1'),
               "Regexp.last_match = nil clears the numbered globals along with $~"
  end
end
