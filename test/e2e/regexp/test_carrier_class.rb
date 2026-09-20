# frozen_string_literal: true

require "test_helper"

# The class a wrapped guest value carries is the one the capability
# registered while its gem installed, so nothing a script does to constant
# lookup reaches it. A match is the probe because the class a script could
# otherwise name is any ordinary class — including the one the bridge
# registers for capability references, whose instances dispatch to the host.
class TestRegexpCarrierClass < Minitest::Test
  include RegexpGuestHelper

  HIJACK_LOOKUP = <<~RUBY
    re = /x/
    def Object.const_get(name); Kobako::Handle; end
    re.match("x").class.to_s
  RUBY

  BIND_OVER_NAME = <<~RUBY
    re = /x/
    Object.const_set(:MatchData, 1)
    re.match("x").class.to_s
  RUBY

  # @behavior RX-182
  def test_replaced_constant_lookup_does_not_name_the_match_class
    assert_equal "MatchData", eval_regexp(HIJACK_LOOKUP),
                 "a match taken after guest code replaces constant lookup must carry " \
                 "the class the capability registered"
  end

  # @behavior RX-183
  def test_value_bound_over_the_match_class_name_leaves_the_match_alone
    assert_equal "MatchData", eval_regexp(BIND_OVER_NAME),
                 "a match taken after guest code binds a non-class over the match " \
                 "class's name must complete and carry the class the capability registered"
  end
end
