# frozen_string_literal: true

require "test_helper"

# Differential parity — `#run` and preloaded snippets (SPEC.md B-31,
# B-32, E-27, E-28, E-32, E-36, E-37, E-38): entrypoint dispatch and
# per-invocation snippet replay must observe identically once the SDK
# grows `run` / `preload`.
class TestParityRunSnippets < Parity::Case
  # SPEC.md B-31 / B-32: preload then dispatch into the entrypoint;
  # snippets replay on every invocation.
  def test_run_entrypoint_pending
    skip "pending SDK run/preload (B-31 B-32)"
  end

  # SPEC.md E-27 / E-28: a missing or non-callable entrypoint.
  def test_entrypoint_faults_pending
    skip "pending SDK run/preload (E-27 E-28)"
  end

  # SPEC.md E-32 / E-36: snippet compile failure and top-level raise
  # at replay.
  def test_snippet_faults_pending
    skip "pending SDK run/preload (E-32 E-36)"
  end

  # SPEC.md E-37 / E-38: RITE version mismatch and corrupt bytecode.
  def test_bytecode_faults_pending
    skip "pending SDK preload(binary:) (E-37 E-38)"
  end
end
