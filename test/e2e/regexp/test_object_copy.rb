# frozen_string_literal: true

require "test_helper"

# dup / clone parity for the CDATA-backed Regexp and MatchData (SPEC.md B-41).
# Both copy methods allocate a bare instance and run initialize_copy on it;
# without a copy body the bare instance carries no payload, so every accessor
# fails. These scenarios pin that the copy owns an independent snapshot.
class TestObjectCopy < Minitest::Test
  include RegexpGuestHelper

  def test_regexp_dup_copies_the_compiled_pattern
    assert_equal ["a(b)c", 1, "b"],
                 eval_regexp('d = /a(b)c/i.dup; [d.source, d.options, d.match("abc")[1]]'),
                 "Regexp#dup carries the source, options, and a working compiled pattern into the copy"
  end

  # clone is a distinct mruby core method from dup; pin that it also routes
  # through initialize_copy so the deeper copy stays a working matcher.
  def test_regexp_clone_copies_the_compiled_pattern
    assert_equal ["a(b)c", 1, "b"],
                 eval_regexp('c = /a(b)c/i.clone; [c.source, c.options, c.match("abc")[1]]'),
                 "Regexp#clone carries the source, options, and a working compiled pattern into the copy"
  end

  # MatchData wraps an owned snapshot (subject, positional and named groups)
  # plus the @regexp ivar; the named capture exercises every MatchState field,
  # so positional [1], named [:g], the subject slice, and #regexp must all
  # survive a dup.
  def test_matchdata_dup_copies_the_match_snapshot
    assert_equal ["b", "b", "x", "a(?<g>b)c"],
                 eval_regexp('d = /a(?<g>b)c/.match("xabcx").dup; [d[1], d[:g], d.pre_match, d.regexp.source]'),
                 "MatchData#dup carries the positional and named groups, subject, and originating regexp into the copy"
  end
end
