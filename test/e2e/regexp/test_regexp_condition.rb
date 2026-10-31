# frozen_string_literal: true

require "test_helper"

# A condition asking after a group by name, which the engine has no spelling
# for. Each expected value is MRI's answer to the same guest code.
class TestRegexpCondition < Minitest::Test
  include RegexpGuestHelper

  # @behavior RX-282
  def test_a_condition_asks_after_a_group_by_name
    assert_equal %w[abc a], eval_regexp('/(?<n>a)?b(?(<n>)c|d)/.match("abc").to_a'),
                 "a condition by name through a match must take the first branch when the group took part"
    assert_equal ["bd", nil], eval_regexp(%q(/(?<n>a)?b(?('n')c|d)/.match("bd").to_a)),
                 "a condition by quoted name through a match must take the second branch when it did not"
    assert_nil eval_regexp('/(?<n>a)?b(?(<n>)c|d)/.match("bc")&.to_a'),
               "a condition by name through a match must not take the branch the group's absence rules out"
  end

  # MRI asks the first group alone, so a part taken only by a later group of
  # the name still answers the second branch.
  # @behavior RX-283
  def test_a_condition_on_a_shared_name_asks_only_its_first_group
    source = "/(?<n>a)?(?<n>b)?(?(<n>)c|d)/"
    {
      "ac" => ["ac", "a", nil], "ad" => ["d", nil, nil], "bd" => ["bd", nil, "b"],
      "abc" => %w[abc a b], "abd" => ["bd", nil, "b"], "d" => ["d", nil, nil]
    }.each do |subject, expected|
      assert_equal expected, eval_regexp(%(#{source}.match("#{subject}").to_a)),
                   "a condition on a shared name through a match on #{subject} must ask the first group"
    end
    assert_nil eval_regexp(%(#{source}.match("bc")&.to_a)),
               "a condition on a shared name through a match on bc must not read the later group as taking part"
  end
end
