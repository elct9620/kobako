# frozen_string_literal: true

require "test_helper"

# How a pattern that cannot compile or cannot be matched cheaply surfaces
# (SPEC.md B-41; docs/regexp.md RX-01). The guest raises RegexpError, which
# reaches an uncatching host as SandboxError; the backtracking ceiling behind
# the match-time half bounds compute without overriding an answer MRI reaches.
class TestRegexpPatternErrors < Minitest::Test
  include RegexpGuestHelper

  # An unbalanced pattern fails to compile; the guest RegexpError surfaces to
  # the host as SandboxError.
  def test_invalid_pattern_raises_sandbox_error
    assert_raises(Kobako::SandboxError,
                  "an invalid pattern surfaces a guest RegexpError as SandboxError") do
      eval_regexp('Regexp.new("(")')
    end
  end

  # A catastrophic-backtracking shape the engine can bound answers as MRI does.
  # The backtracking ceiling guards the invocation's wall-clock budget, so it
  # must not turn an answer MRI reaches into an error.
  def test_catastrophic_backtracking_answers_no_match_like_mri
    assert_nil eval_regexp('/(a+)+\1$/.match("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaa!")'),
               "a nested-quantifier backreference through Regexp#match must answer nil, as MRI does"
  end

  # A match the engine cannot bound reaches the backtracking ceiling; left
  # uncaught, that guest RegexpError surfaces to the host as SandboxError just
  # as a compile-time one does.
  def test_unbounded_match_raises_sandbox_error
    assert_raises(Kobako::SandboxError,
                  "a match past the backtracking ceiling surfaces a guest RegexpError as SandboxError") do
      eval_regexp('/(a|aa)+\1$/.match("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaa!")')
    end
  end

  # The RegexpError diagnostic quotes the pattern; quoting the subject instead
  # would mislabel user data as the invalid expression.
  def test_match_time_engine_error_names_the_pattern
    message = eval_regexp('begin; /(a|aa)+\1$/.match("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaa!"); "matched"; ' \
                          "rescue RegexpError => e; e.message; end")

    assert_includes message, "(a|aa)+",
                    "a match-time engine error through Regexp#match must name the pattern source"
    refute_includes message, "aaaaaaaa",
                    "a match-time engine error through Regexp#match must not embed the subject"
  end

  # The gem provides RegexpError as a StandardError subclass, so guest code
  # can rescue a bad pattern with a bare rescue or rescue StandardError.
  def test_regexp_error_is_a_standard_error
    assert_equal true, eval_regexp("RegexpError.ancestors.include?(StandardError)"),
                 "RegexpError is a StandardError subclass guest code can rescue"
  end
end
