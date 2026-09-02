# frozen_string_literal: true

require "test_helper"

# Differential parity — `#run` and preloaded snippets (SPEC.md B-31,
# B-32, E-27, E-28, E-32, E-36, E-37, E-38): entrypoint dispatch and
# per-invocation snippet replay must observe identically through both
# frontends.
class TestParityRunSnippets < Parity::Case
  BYTECODE_ANSWERS_HEX = File.binread(
    File.expand_path("../fixtures/snippet_answers.mrb", __dir__)
  ).unpack1("H*")

  # The B-31/B-32 replay chain: the Total snippet evaluates its
  # predecessors' constants (one of them bytecode-form) at replay time,
  # so an insertion-order or replay drift fails the whole chain.
  REPLAY_CHAIN = [
    { kind: "source", name: "Base", code: "BASE = 40" },
    { kind: "bytecode", hex: BYTECODE_ANSWERS_HEX },
    { kind: "source", name: "Total", code: "TOTAL = BASE + ANSWERS" },
    { kind: "source", name: "Handler", code: "Handler = ->() { TOTAL }" }
  ].freeze

  # SPEC.md B-31 / B-32: dispatch into a preloaded entrypoint; source
  # and bytecode snippets replay in insertion order on every
  # invocation, uniformly across the #run / #eval verbs.
  # @behavior S-050
  def test_run_entrypoint
    assert_parity Parity::Scenario.new(
      name: "run-entrypoint", anchors: %w[B-31 B-32],
      preloads: REPLAY_CHAIN,
      invocations: [
        { verb: "run", target: "Handler" },
        { verb: "run", target: "Handler" },
        { verb: "eval", source: "TOTAL" }
      ]
    )
  end

  # @behavior S-082
  # Both entrypoint faults are sandbox-origin failures rather than traps,
  # and a frontend that attributed either as a trap would tell its Host
  # App the guest had been cut short when it had not.
  def test_entrypoint_faults
    assert_parity Parity::Scenario.new(
      name: "entrypoint-faults", anchors: %w[E-27 E-28],
      preloads: [{ kind: "source", name: "NotCallable", code: "NotCallable = 8" }],
      invocations: [
        { verb: "run", target: "Missing" },
        { verb: "run", target: "NotCallable" }
      ]
    )
  end

  # @behavior S-091
  # Both snippet faults surface on the invocation that replays them
  # rather than on the preload, so a frontend reporting either early
  # would be answering before the guest had read the snippet at all.
  def test_snippet_faults
    assert_parity Parity::Scenario.new(
      name: "snippet-compile-failure", anchors: %w[E-32],
      preloads: [{ kind: "source", name: "Broken", code: "def broken(" }],
      invocations: [{ verb: "eval", source: "1" }]
    )
    assert_parity Parity::Scenario.new(
      name: "snippet-replay-raise", anchors: %w[E-36],
      preloads: [{ kind: "source", name: "Boom", code: 'raise "boom at replay"' }],
      invocations: [{ verb: "eval", source: "1" }]
    )
  end

  # SPEC.md E-37 / E-38: RITE version mismatch and corrupt bytecode are
  # the two structural failure modes reserved for the bytecode status;
  # fixtures are the e2e suite's flipped-version and truncated blobs.
  def test_bytecode_faults
    %w[snippet_wrong_version snippet_corrupt].each do |fixture|
      hex = File.binread(File.expand_path("../fixtures/#{fixture}.mrb", __dir__)).unpack1("H*")
      assert_parity Parity::Scenario.new(
        name: fixture.tr("_", "-"), anchors: %w[E-37 E-38],
        preloads: [{ kind: "bytecode", hex: hex }],
        invocations: [{ verb: "eval", source: "nil" }]
      )
    end
  end
end
