# frozen_string_literal: true

require "test_helper"

# Error and Enumerator behaviour of scan / gsub / sub: a block that raises
# propagates to the caller; gsub without a block or a replacement yields an
# Enumerator via to_enum (which the curated guest only provides when
# mruby-enumerator is added), while sub requires a block or a replacement.
class TestRegexpSubstitutionErrors < Minitest::Test
  include RegexpGuestHelper

  # @behavior RX-163
  def test_scan_propagates_block_exception
    assert_equal "boom",
                 eval_regexp('begin; "aa".scan(/a/){ raise "boom" }; "swallowed"; ' \
                             "rescue => e; e.message; end"),
                 "an exception raised in a scan block propagates to the caller"
  end

  # @behavior RX-164
  def test_gsub_propagates_block_exception
    assert_equal "boom",
                 eval_regexp('begin; "aa".gsub(/a/){ raise "boom" }; "swallowed"; ' \
                             "rescue => e; e.message; end"),
                 "an exception raised in a gsub block propagates to the caller"
  end

  # @behavior RX-165
  def test_gsub_without_block_or_replacement_delegates_to_to_enum
    # gsub now delegates to to_enum (rather than silently substituting ""); the
    # curated guest has no Fiber, so building the Enumerator fails loudly. A
    # guest that adds mruby-enumerator gets the real Enumerator instead.
    error = assert_raises(Kobako::SandboxError) { eval_regexp('"aa".gsub(/a/)') }
    assert_match(/enumerator/, error.message,
                 "gsub with neither a block nor a replacement delegates to to_enum (an Enumerator)")
  end

  # @behavior RX-166
  def test_sub_without_block_or_replacement_raises_argument_error
    assert_equal "ArgumentError", guard_error('"aa".sub(/a/)', "ArgumentError"),
                 "sub with neither a block nor a replacement raises ArgumentError"
  end

  # The subject holds no match, so a refusal can only come from the
  # replacement being checked before the search, as the language does.
  # @behavior RX-226
  def test_a_replacement_that_is_neither_a_string_nor_a_hash_is_refused
    %w[1 nil].each do |replacement|
      %w[sub gsub].each do |method|
        assert_equal "TypeError", guard_error("'b'.#{method}(/a/, #{replacement})", "TypeError"),
                     "#{replacement} as the replacement through String##{method} must raise TypeError"
      end
    end
  end
end
