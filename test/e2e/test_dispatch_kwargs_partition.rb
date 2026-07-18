# frozen_string_literal: true

require "test_helper"

# E2E (Layer 4) — the guest→host dispatch partitions positional and keyword
# arguments by Ruby 3 call semantics (SPEC.md B-58), not the Ruby 2
# trailing-Hash fold: a brace-less +key: value+ reaches the Service as a
# keyword argument, while an explicit +{...}+ Hash literal in positional
# position stays positional. Each sink Service captures +args+ and +kwargs+
# separately into a host-side closure so the partition the guest bridge
# produced is observable after the dispatch, independent of the
# wire-representable value it returns.
class TestE2EDispatchKwargsPartition < Minitest::Test
  include E2eGuestHelper

  def test_braceless_keyword_argument_arrives_as_a_keyword_at_the_service
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    captured = nil
    sandbox.bind("Rpc::Sink", ->(*args, **kwargs) { captured = { args: args, kwargs: kwargs } })

    sandbox.eval('Rpc::Sink.call("u", a: 1)')

    assert_equal ["u"], captured[:args],
                 "B-58: a brace-less kwarg must leave the positional args untouched"
    assert_equal({ a: 1 }, captured[:kwargs],
                 "B-58: a brace-less key: value must arrive at the Service as a keyword argument")
  end

  def test_explicit_positional_hash_stays_a_positional_argument
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    captured = nil
    sandbox.bind("Rpc::Sink", ->(*args, **kwargs) { captured = { args: args, kwargs: kwargs } })

    sandbox.eval('Rpc::Sink.call("u", {a: 1})')

    assert_equal ["u", { a: 1 }], captured[:args],
                 "B-58: an explicit {...} Hash literal must stay positional, never fold into kwargs"
    assert_equal({}, captured[:kwargs],
                 "B-58: no brace-less keyword must reach the Service with empty kwargs, even with a trailing Hash")
  end

  def test_positional_hash_and_braceless_keyword_stay_in_disjoint_buckets
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    captured = nil
    sandbox.bind("Rpc::Sink", ->(*args, **kwargs) { captured = { args: args, kwargs: kwargs } })

    sandbox.eval('Rpc::Sink.call("u", {a: 1}, b: 2)')

    assert_equal ["u", { a: 1 }], captured[:args],
                 "B-58: with both present, the explicit Hash stays positional and does not absorb the keyword"
    assert_equal({ b: 2 }, captured[:kwargs],
                 "B-58: with both present, the brace-less keyword stays in kwargs and ignores the positional Hash")
  end
end
