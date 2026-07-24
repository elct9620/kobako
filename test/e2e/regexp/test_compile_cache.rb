# frozen_string_literal: true

require "test_helper"

# Per-invocation compiled-pattern memoization (SPEC.md B-41 / docs/regexp.md
# RX-08). The cache reuses a compiled pattern across repeated compilation of the
# same (source, options) within one invocation, yet stays observably invisible.
# These scenarios pin that invisibility — distinct objects, options as part of
# the key, and correct matching past the bounded capacity — so a regression that
# shared the wrong engine or collided keys would surface as a wrong result.
class TestCompileCache < Minitest::Test
  include RegexpGuestHelper

  # Far more distinct patterns than the 64-entry default capacity, each checked
  # against its own subject and a non-match, so an eviction-and-recompile cycle
  # that corrupted a key or shared the wrong engine would fail a match.
  CAPACITY_STRESS = <<~'RUBY'
    ok = true
    100.times do |i|
      re = Regexp.new("val#{i}")
      ok &&= re.match?("val#{i}")
      ok &&= !re.match?("zzz")
    end
    ok
  RUBY

  # A literal recompiled in a hot loop — the shape the cache targets.
  HOT_LOOP = <<~RUBY
    hits = 0
    1000.times { hits += 1 if /foo|bar|baz/ =~ "the quick brown bar jumps" }
    hits
  RUBY

  # Memoization shares the engine, never the object: each literal evaluation
  # still allocates its own Regexp, as in mruby and the original C engine.
  def test_repeated_literal_yields_distinct_objects
    assert_equal false,
                 eval_regexp("/a(b)c/.equal?(/a(b)c/)"),
                 "the same literal compiled twice must remain two distinct Regexp objects"
  end

  # The cache key carries the option bits, so an identical source compiled with
  # a different flag must not borrow the first engine.
  def test_same_source_different_options_do_not_collide
    assert_equal [true, false],
                 eval_regexp('[/a/i.match?("A"), /a/.match?("A")]'),
                 "a case-insensitive pattern and its case-sensitive twin must each match on their own terms"
  end

  # Eviction past the bounded capacity changes throughput, never correctness.
  def test_matches_correctly_past_cache_capacity
    assert_equal true,
                 eval_regexp(CAPACITY_STRESS),
                 "every one of 100 distinct patterns must match its own subject regardless of cache eviction"
  end

  # The memoized hot-loop literal reports the same count as an unmemoized one.
  def test_hot_loop_literal_matches_correctly
    assert_equal 1000,
                 eval_regexp(HOT_LOOP),
                 "a literal matched 1000 times must report 1000 hits whether or not its engine is memoized"
  end
end
