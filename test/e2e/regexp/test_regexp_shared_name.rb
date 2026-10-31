# frozen_string_literal: true

require "test_helper"

# A name several groups share reads one group per match: the last that took
# part, as MRI answers. Each scenario drives both alternatives or both
# sequential groups, so an implementation reading only the first or only the
# last group cannot pass.
class TestRegexpSharedName < Minitest::Test
  include RegexpGuestHelper

  ALTERNATIVES = "/(?<n>a)|(?<n>b)/"

  # @behavior RX-257
  def test_a_shared_name_reads_the_alternative_that_matched
    assert_equal "a", eval_regexp(%(#{ALTERNATIVES}.match("a")[:n])),
                 "a shared name through MatchData#[] must read the first alternative when it matched"
    assert_equal "b", eval_regexp(%(#{ALTERNATIVES}.match("b")["n"])),
                 "a shared name through MatchData#[] must read the second alternative when it matched"
  end

  # @behavior RX-257
  def test_a_shared_name_reads_the_last_group_that_took_part
    assert_equal "b", eval_regexp('/(?<n>a)(?<n>b)/.match("ab")[:n]'),
                 "a shared name through MatchData#[] must read the last of two groups that both matched"
    assert_equal "a", eval_regexp('/(?<n>a)(?<n>b)?/.match("a")[:n]'),
                 "a shared name through MatchData#[] must skip a later group that did not take part"
    assert_nil eval_regexp('/(?<n>a)?(?<n>b)?c/.match("c")[:n]'),
               "a shared name through MatchData#[] must read nil when none of its groups took part"
  end

  # @behavior RX-257
  def test_a_shared_name_locates_the_group_that_took_part
    assert_equal [1, 2, [1, 2]],
                 eval_regexp(%(m = #{ALTERNATIVES}.match("xb"); [m.begin(:n), m.end(:n), m.offset(:n)])),
                 "a shared name through MatchData#begin, #end and #offset must locate the group that took part"
  end

  # @behavior RX-257
  def test_a_string_slice_reads_a_shared_name_by_the_group_that_took_part
    assert_equal "b", eval_regexp(%("b"[#{ALTERNATIVES}, "n"])),
                 "a shared name through String#[] must read the group that took part"
  end

  # @behavior RX-258
  def test_named_captures_read_a_shared_name_by_the_group_that_took_part
    assert_equal({ "n" => "a" }, eval_regexp(%(#{ALTERNATIVES}.match("a").named_captures)),
                 "a shared name through MatchData#named_captures must not let an idle later group blank it")
    assert_equal({ "n" => "b" }, eval_regexp(%(#{ALTERNATIVES}.match("b").named_captures)),
                 "a shared name through MatchData#named_captures must read the second alternative when it matched")
  end

  # @behavior RX-258
  def test_named_captures_key_a_shared_name_once_where_first_declared
    assert_equal [%w[a z], %w[b y]],
                 eval_regexp('/(?<a>x)(?<b>y)(?<a>z)/.match("xyz").named_captures.to_a'),
                 "a shared name through MatchData#named_captures must keep the place it was first declared"
  end

  # @behavior RX-259
  def test_a_match_lists_a_shared_name_once
    assert_equal %w[a b], eval_regexp('/(?<a>x)(?<b>y)(?<a>z)/.match("xyz").names'),
                 "a shared name through MatchData#names must come back once, where it was first declared"
  end

  # @behavior RX-260
  def test_a_replacement_expands_a_shared_name_per_match
    assert_equal "[a][b]", eval_regexp(%("ab".gsub(#{ALTERNATIVES}, '[\\k<n>]'))),
                 "a shared name through String#gsub's replacement must expand the alternative each match took"
    assert_equal "[b]", eval_regexp(%q("ab".sub(/(?<n>a)(?<n>b)/, '[\k<n>]'))),
                 "a shared name through String#sub's replacement must expand the last group that took part"
  end

  # @behavior RX-261
  def test_a_pattern_maps_a_shared_name_to_every_group
    assert_equal [["a", [1, 3]], ["b", [2]]],
                 eval_regexp("/(?<a>x)(?<b>y)(?<a>z)/.named_captures.to_a"),
                 "a shared name through Regexp#named_captures must map to every group, keeping the declared order"
  end

  # @behavior RX-262
  def test_a_pattern_lists_a_shared_name_once
    assert_equal %w[a b], eval_regexp("/(?<a>x)(?<b>y)(?<a>z)/.names"),
                 "a shared name through Regexp#names must come back once, where it was first declared"
  end

  # @behavior RX-274
  def test_a_backreference_by_a_shared_name_tries_each_group
    source = '/(?<n>a)(?<n>b)\k<n>/'

    assert_equal %w[abb a b], eval_regexp(%(#{source}.match("abb").to_a)),
                 "a shared name through a backreference must match the last group's text"
    assert_equal %w[aba a b], eval_regexp(%(#{source}.match("aba").to_a)),
                 "a shared name through a backreference must match an earlier group's text"
    assert_nil eval_regexp(%(#{source}.match("abc")&.to_a)),
               "a shared name through a backreference must fail when no group's text follows"
  end

  # @behavior RX-274
  def test_a_backreference_by_a_shared_name_skips_a_group_that_did_not_take_part
    assert_equal ["aa", "a", nil], eval_regexp('/(?:(?<n>a)|(?<n>b))\k<n>/.match("aa").to_a'),
                 "a shared name through a backreference must read the first alternative when it took part"
    assert_equal ["bb", nil, "b"], eval_regexp('/(?:(?<n>a)|(?<n>b))\k<n>/.match("bb").to_a'),
                 "a shared name through a backreference must read the second alternative when it took part"
    assert_equal %w[aab a b], eval_regexp('/(?<n>a)\k<n>(?<n>b)/.match("aab").to_a'),
                 "a shared name through a backreference must reach a group declared after it"
  end

  # The last group's text matches here, but what follows it does not; the
  # earlier group's text would let the whole match succeed, and MRI never
  # returns to try it.
  # @behavior RX-275
  def test_a_backreference_by_a_shared_name_settles_on_the_first_group_that_matches
    assert_nil eval_regexp('/(?<n>ab)(?<n>a)\k<n>c/.match("abaabc")&.to_a'),
               "a shared name through a backreference must not return to try an earlier group"
  end

  # @behavior RX-276
  def test_a_subexpression_call_by_a_shared_name_is_refused
    assert_equal "RegexpError", guard_error('/(?<n>a)(?<n>b)\g<n>/.match("ab")', "RegexpError"),
                 "a shared name through \\g must raise RegexpError"
    assert_equal %w[aa a], eval_regexp('/(?<n>a)\g<n>/.match("aa").to_a'),
                 "an unshared name through \\g must still call its group"
  end
end
