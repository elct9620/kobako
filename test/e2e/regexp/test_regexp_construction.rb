# frozen_string_literal: true

require "test_helper"

# What a pattern can be built from. Rendering any other value as text
# would compile it, and nil would become the empty pattern that matches
# everywhere, so each is refused instead.
class TestRegexpConstruction < Minitest::Test
  include RegexpGuestHelper

  # @behavior RX-227
  def test_a_source_that_is_neither_a_string_nor_a_pattern_is_refused
    %w[1 :a nil].each do |source|
      %w[new compile].each do |name|
        assert_equal "TypeError", guard_error("Regexp.#{name}(#{source})", "TypeError"),
                     "#{source} as the source through Regexp.#{name} must raise TypeError"
      end
    end
  end
end
