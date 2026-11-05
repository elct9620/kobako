# frozen_string_literal: true

require "test_helper"

# Differential parity — the Extension install mechanism. Both frontends
# install a guest Storage idiom over a stub backend and must observe the
# composition and provider resolution identically: a pure method runs
# in-guest, an I/O method dispatches to the backend, a fixed provider
# persists a stateful backend across invocations, and a per-invocation
# provider resets it.
class TestParityInstall < Parity::Case
  PURE_AND_IO = "class Storage; extend Kobako::Proxy; def self.join(*p); p.join('/'); end; end"
  BACKED_ONLY = "class Storage; extend Kobako::Proxy; end"

  # @behavior EX-005 EX-006 EX-038
  # Storage.join runs in-guest with no round-trip; Storage.read dispatches to
  # the bound backend.
  def test_pure_method_is_local_and_io_dispatches_to_the_backend
    assert_install(
      name: "install-composition", source: PURE_AND_IO,
      backend: { path: "Storage", provider: "fixed", methods: { read: { behavior: "echo" } } },
      sources: ["Storage.join('dir', 'a.txt')", "Storage.read('payload')"]
    )
  end

  # @behavior EX-021 EX-039
  # A fixed provider binds one backend for the Sandbox's life, so a
  # counter keeps counting across invocations.
  def test_fixed_provider_persists_a_stateful_backend
    assert_install(
      name: "install-fixed-provider", source: BACKED_ONLY,
      backend: counter_backend("fixed"), sources: %w[Storage.tick Storage.tick]
    )
  end

  # @behavior EX-022 EX-040
  # A per-invocation provider resolves a fresh backend each invocation, so
  # the counter resets — the write cannot leak across invocations.
  def test_per_invocation_provider_resets_a_stateful_backend
    assert_install(
      name: "install-per-invocation-provider", source: BACKED_ONLY,
      backend: counter_backend("per_invocation"), sources: %w[Storage.tick Storage.tick]
    )
  end

  private

  def assert_install(name:, source:, backend:, sources:)
    assert_parity Parity::Scenario.new(
      name:,
      extensions: [{ name: "Storage", source: source, backend: backend }],
      invocations: sources.map { |code| { verb: "eval", source: code } }
    )
  end

  def counter_backend(provider)
    { path: "Storage", provider: provider, methods: { tick: { behavior: "counter" } } }
  end
end
