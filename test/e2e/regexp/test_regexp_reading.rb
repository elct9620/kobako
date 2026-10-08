# frozen_string_literal: true

require "test_helper"

# How the pattern methods read a String or Symbol they are handed. A guest
# may redefine String#to_s or Symbol#to_s; the language reads a String's own
# characters and a Symbol's own name, so neither redefinition may reach them.
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

  SYMBOL_RENDERING_REDEFINED = <<~RUBY
    class Symbol; def to_s = "zzz"; end
    [/a/.match?(:a), Regexp.escape(:"a.b"), /(?<n>a)/.match("a").begin(:n)]
  RUBY

  # @behavior RX-234
  def test_a_symbol_is_read_as_its_own_name
    assert_equal [true, 'a\.b', 0], eval_regexp(SYMBOL_RENDERING_REDEFINED),
                 "a Symbol through Regexp#match?, Regexp.escape and MatchData#begin " \
                 "must be read as its own name, not through a redefined Symbol#to_s"
  end
end
