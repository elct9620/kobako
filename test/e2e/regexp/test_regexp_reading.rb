# frozen_string_literal: true

require "test_helper"

# How the pattern methods read a String they are handed. A guest may
# redefine String#to_s; the language reads a String's own characters, so
# the redefinition must not reach any of them.
class TestRegexpReading < Minitest::Test
  include RegexpGuestHelper

  RENDERING_REDEFINED = <<~RUBY
    class String; def to_s = "zzz"; end
    [/a/.match?("a"), "abc".scan("b"), Regexp.new("a").source, Regexp.escape("a."), "a".sub("a", "x")]
  RUBY

  # @behavior RX-233
  def test_a_string_is_read_as_its_own_characters
    assert_equal [true, ["b"], "a", 'a\.', "x"], eval_regexp(RENDERING_REDEFINED),
                 "a String through Regexp#match?, String#scan, Regexp.new, Regexp.escape and String#sub " \
                 "must be read as its own characters, not through a redefined String#to_s"
  end
end
