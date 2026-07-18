# frozen_string_literal: true

require "test_helper"

# E2E (Layer 4) — the guest→host dispatch partitions positional and keyword
# arguments by Ruby 3 call semantics (SPEC.md B-58), not the Ruby 2
# trailing-Hash fold. Each case drives a real dispatch through a sink Service
# that captures +args+ and +kwargs+ separately into a host-side ivar, so the
# partition the guest bridge produced is observable after the call. The cases
# span the kwargs call-site variants: brace-less keywords, an explicit +{...}+
# positional literal, the +**+ double splat, and their empty and mixed forms.
class TestE2EDispatchKwargsPartition < Minitest::Test
  include E2eGuestHelper

  def setup
    super
    @sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    @sandbox.bind("Rpc::Sink", ->(*args, **kwargs) { @captured = { args: args, kwargs: kwargs } })
  end

  def test_braceless_keyword_argument_arrives_as_a_keyword_at_the_service
    @sandbox.eval('Rpc::Sink.call("u", a: 1)')

    assert_equal ["u"], @captured[:args],
                 "B-58: a brace-less kwarg must leave the positional args untouched"
    assert_equal({ a: 1 }, @captured[:kwargs],
                 "B-58: a brace-less key: value must arrive at the Service as a keyword argument")
  end

  def test_explicit_positional_hash_stays_a_positional_argument
    @sandbox.eval('Rpc::Sink.call("u", {a: 1})')

    assert_equal ["u", { a: 1 }], @captured[:args],
                 "B-58: an explicit {...} Hash literal must stay positional, never fold into kwargs"
    assert_equal({}, @captured[:kwargs],
                 "B-58: no brace-less keyword must reach the Service with empty kwargs, even with a trailing Hash")
  end

  def test_positional_hash_and_braceless_keyword_stay_in_disjoint_buckets
    @sandbox.eval('Rpc::Sink.call("u", {a: 1}, b: 2)')

    assert_equal ["u", { a: 1 }], @captured[:args],
                 "B-58: with both present, the explicit Hash stays positional and does not absorb the keyword"
    assert_equal({ b: 2 }, @captured[:kwargs],
                 "B-58: with both present, the brace-less keyword stays in kwargs and ignores the positional Hash")
  end

  def test_double_splat_hash_arrives_as_keyword_arguments
    @sandbox.eval('opts = {a: 1}; Rpc::Sink.call("u", **opts)')

    assert_equal ["u"], @captured[:args],
                 "B-58: a ** double-splat must leave the positional args untouched"
    assert_equal({ a: 1 }, @captured[:kwargs],
                 "B-58: a ** double-splat of a Hash must arrive at the Service as keyword arguments")
  end

  def test_empty_double_splat_produces_no_keyword_arguments
    @sandbox.eval('Rpc::Sink.call("u", **{})')

    assert_equal ["u"], @captured[:args],
                 "B-58: an empty ** double-splat must not manufacture a positional argument"
    assert_equal({}, @captured[:kwargs],
                 "B-58: an empty ** double-splat must reach the Service with empty kwargs")
  end

  def test_hash_valued_keyword_stays_a_keyword_with_its_hash_value
    @sandbox.eval('Rpc::Sink.call("u", data: {n: 1})')

    assert_equal ["u"], @captured[:args],
                 "B-58: a keyword whose value is a Hash must not leak into the positional args"
    assert_equal({ data: { n: 1 } }, @captured[:kwargs],
                 "B-58: a Hash-valued keyword must arrive as a keyword carrying its Hash value intact")
  end

  def test_empty_positional_hash_stays_a_positional_argument
    @sandbox.eval('Rpc::Sink.call("u", {})')

    assert_equal ["u", {}], @captured[:args],
                 "B-58: an empty {} Hash literal must stay a positional argument, not vanish into kwargs"
    assert_equal({}, @captured[:kwargs],
                 "B-58: an empty {} positional literal must not populate kwargs")
  end
end
