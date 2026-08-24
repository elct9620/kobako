# frozen_string_literal: true

require "test_helper"

# How a pattern that cannot compile, or a match that cannot be bounded,
# surfaces (SPEC.md B-41; docs/regexp.md RX-01 / RX-10). The guest raises
# RegexpError, which reaches an uncatching host as SandboxError. The witness
# for the ceiling is a shape MRI cannot answer either, so the tests pin the
# contract rather than the engine version's threshold.
class TestRegexpPatternErrors < Minitest::Test
  include RegexpGuestHelper

  # An unbalanced pattern fails to compile; the guest RegexpError surfaces to
  # the host as SandboxError.
  # @behavior RX-076
  def test_invalid_pattern_raises_sandbox_error
    assert_raises(Kobako::SandboxError,
                  "an invalid pattern surfaces a guest RegexpError as SandboxError") do
      eval_regexp('Regexp.new("(")')
    end
  end

  # A catastrophic-backtracking shape the engine can bound answers as MRI does.
  # The backtracking ceiling guards the invocation's wall-clock budget, so it
  # must not turn an answer MRI reaches into an error.
  # @behavior RX-077
  def test_catastrophic_backtracking_answers_no_match_like_mri
    assert_nil eval_regexp('/(a+)+\1$/.match("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaa!")'),
               "a nested-quantifier backreference through Regexp#match must answer nil, as MRI does"
  end

  # The witness leaves MRI running for over three minutes without an answer,
  # so reaching the ceiling costs nothing MRI could have delivered. Left
  # uncaught, that guest RegexpError surfaces to the host as SandboxError just
  # as a compile-time one does.
  # @behavior RX-078
  def test_unbounded_match_raises_sandbox_error
    assert_raises(Kobako::SandboxError,
                  "a match MRI cannot answer either surfaces a guest RegexpError as SandboxError") do
      eval_regexp('/(a|aa|aaa)+\1$/.match("a" * 40 + "!")')
    end
  end

  # The RegexpError diagnostic quotes the pattern; quoting the subject instead
  # would mislabel user data as the invalid expression.
  # @behavior RX-079 RX-080
  def test_match_time_engine_error_names_the_pattern
    message = eval_regexp('begin; /(a|aa|aaa)+\1$/.match("a" * 40 + "!"); "matched"; ' \
                          "rescue RegexpError => e; e.message; end")

    assert_includes message, "(a|aa|aaa)+",
                    "a match-time engine error through Regexp#match must name the pattern source"
    refute_includes message, "aaaaaaaa",
                    "a match-time engine error through Regexp#match must not embed the subject"
  end

  # The gem provides RegexpError as a StandardError subclass, so guest code
  # can rescue a bad pattern with a bare rescue or rescue StandardError.
  # @behavior RX-081
  def test_regexp_error_is_a_standard_error
    assert_equal true, eval_regexp("RegexpError.ancestors.include?(StandardError)"),
                 "RegexpError is a StandardError subclass guest code can rescue"
  end
end
