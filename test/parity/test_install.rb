# frozen_string_literal: true

require "test_helper"

# Differential parity — the Extension install mechanism (SPEC.md
# B-55 / B-56). Both frontends install a guest File idiom over a stub
# backend and must observe the composition and provider resolution
# identically: a pure method runs in-guest, an I/O method dispatches to the
# backend, a fixed provider persists a stateful backend across invocations,
# and a per-invocation provider resets it.
class TestParityInstall < Parity::Case
  PURE_AND_IO = "class File; extend Kobako::Proxy; def self.join(*p); p.join('/'); end; end"
  BACKED_ONLY = "class File; extend Kobako::Proxy; end"

  # B-55: File.join runs in-guest with no round-trip; File.read dispatches
  # to the bound backend.
  def test_pure_method_is_local_and_io_dispatches_to_the_backend
    assert_install(
      name: "install-composition", anchors: %w[B-55], source: PURE_AND_IO,
      backend: { path: "File", provider: "fixed", methods: { read: { behavior: "echo" } } },
      sources: ["File.join('dir', 'a.txt')", "File.read('payload')"]
    )
  end

  # B-56: a fixed provider binds one backend for the Sandbox's life, so a
  # counter keeps counting across invocations.
  def test_fixed_provider_persists_a_stateful_backend
    assert_install(
      name: "install-fixed-provider", anchors: %w[B-56], source: BACKED_ONLY,
      backend: counter_backend("fixed"), sources: %w[File.tick File.tick]
    )
  end

  # B-56: a per-invocation provider resolves a fresh backend each
  # invocation, so the counter resets — the write cannot leak across
  # invocations.
  def test_per_invocation_provider_resets_a_stateful_backend
    assert_install(
      name: "install-per-invocation-provider", anchors: %w[B-56], source: BACKED_ONLY,
      backend: counter_backend("per_invocation"), sources: %w[File.tick File.tick]
    )
  end

  private

  def assert_install(name:, anchors:, source:, backend:, sources:)
    assert_parity Parity::Scenario.new(
      name: name, anchors: anchors,
      extensions: [{ name: "File", source: source, backend: backend }],
      invocations: sources.map { |code| { verb: "eval", source: code } }
    )
  end

  def counter_backend(provider)
    { path: "File", provider: provider, methods: { tick: { behavior: "counter" } } }
  end
end
