# frozen_string_literal: true

require "test_helper"

# A capability gem calling back into guest code mid-operation, on the
# regexp surface: a Hash replacement whose lookup raises during a
# substitution. The raise has to stay a guest exception the caller may
# rescue, never a trap that retires the Sandbox.
class TestRegexpRaisingLookup < Minitest::Test
  include RegexpGuestHelper

  RAISING_LOOKUP = <<~RUBY
    table = Hash.new { |_, key| raise "no replacement for \#{key}" }
    begin
      "abc".gsub(/b/, table)
    rescue RuntimeError => e
      e.message
    end
  RUBY

  # @behavior MR-012
  def test_a_raising_hash_lookup_during_substitution_is_rescuable
    assert_equal "no replacement for b", eval_regexp(RAISING_LOOKUP),
                 "a raise inside a Hash replacement's lookup during String#gsub must be a guest " \
                 "exception the caller may rescue, never a trap"
  end
end
