# frozen_string_literal: true

require "test_helper"

# Error and Enumerator behaviour of scan / gsub / sub (SPEC.md B-41): a block
# that raises propagates to the caller; gsub without a block or a replacement
# yields an Enumerator via to_enum (which the curated guest only provides when
# mruby-enumerator is added), while sub requires a block or a replacement.
class TestRegexpSubstitutionErrors < Minitest::Test
  include RegexpGuestHelper

  def test_scan_propagates_block_exception
    assert_equal "boom",
                 eval_regexp('begin; "aa".scan(/a/){ raise "boom" }; "swallowed"; ' \
                             "rescue => e; e.message; end"),
                 "an exception raised in a scan block propagates to the caller"
  end

  def test_gsub_propagates_block_exception
    assert_equal "boom",
                 eval_regexp('begin; "aa".gsub(/a/){ raise "boom" }; "swallowed"; ' \
                             "rescue => e; e.message; end"),
                 "an exception raised in a gsub block propagates to the caller"
  end

  def test_gsub_without_block_or_replacement_delegates_to_to_enum
    # gsub now delegates to to_enum (rather than silently substituting ""); the
    # curated guest has no Fiber, so building the Enumerator fails loudly. A
    # guest that adds mruby-enumerator gets the real Enumerator instead.
    error = assert_raises(Kobako::SandboxError) { eval_regexp('"aa".gsub(/a/)') }
    assert_match(/enumerator/, error.message,
                 "gsub with neither a block nor a replacement delegates to to_enum (an Enumerator)")
  end

  def test_sub_without_block_or_replacement_raises_argument_error
    assert_equal "ArgumentError",
                 eval_regexp('begin; "aa".sub(/a/); "substituted"; ' \
                             'rescue ArgumentError; "ArgumentError"; rescue => e; e.class.to_s; end'),
                 "sub with neither a block nor a replacement raises ArgumentError"
  end
end
