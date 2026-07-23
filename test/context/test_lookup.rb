# frozen_string_literal: true

# Layer-3 unit test for Kobako::Context#lookup — the per-invocation path
# resolver the dispatch handler routes each guest→host String target through.
# Pure Ruby; does NOT require the native extension. #lookup never touches the
# Runtime, so a bare placeholder stands in for that unused collaborator.
#
# The overlay's positive path — a callable backend's fresh per-invocation
# object reaching the guest — is driven end-to-end in test/e2e/test_install.rb
# through a real guest. This file pins the two host-side invariants that live
# on #lookup: static base delegation, and the bounded overlay the removed
# Services#refresh no-op test used to guard — per-invocation resolution
# overlays the static bindings and never makes an unbound path reachable.
#
# Cross-references:
#   - SPEC.md / docs/behavior/extension.md B-56 — a backend is fixed or
#     resolved fresh per invocation; provider identity is resource identity
#   - SPEC.md / docs/behavior/dispatch.md B-33 — the bound path set is fixed
#     at the seal; per-invocation resolution cannot grow it

require "test_helper"

module Kobako
  class ContextLookupTest < Minitest::Test
    def setup
      @services = Kobako::Catalog::Services.new
      @snippets = Kobako::Catalog::Snippets.new
      @extensions = Kobako::Catalog::Extensions.new
    end

    def context
      Kobako::Context.new(runtime: Object.new, services: @services,
                          snippets: @snippets, extensions: @extensions)
    end

    def test_lookup_resolves_a_bound_path_to_its_base_object
      kv = Object.new
      @services.bind("Store::KV", kv)

      assert_same kv, context.lookup("Store::KV"),
                  "lookup through a Context must resolve a statically-bound path to its base object (B-56)"
    end

    def test_lookup_raises_key_error_for_a_never_bound_path
      @services.bind("Store::KV", Object.new)

      assert_raises(KeyError,
                    "lookup on a never-bound path must raise, so per-invocation resolution overlays the " \
                    "static bindings and never makes an unbound path reachable — the key set sealed at the " \
                    "first invocation cannot grow (B-33)") do
        context.lookup("Store::Missing")
      end
    end

    # B-62: a fillable declared with bind(path) is backed by the shared
    # Kobako::Unresolved sentinel; lookup reports it as unresolvable (KeyError)
    # so the dispatch fails closed as an undefined target rather than
    # dispatching to the sentinel itself.
    def test_lookup_reports_an_unfilled_fillable_as_unresolvable
      @services.bind("Store", Kobako::Unresolved)

      assert_raises(KeyError,
                    "lookup on a fillable left unfilled must raise, so an unresolved capability fails " \
                    "closed as an undefined target instead of dispatching to Kobako::Unresolved (B-62)") do
        context.lookup("Store")
      end
    end
  end
end
