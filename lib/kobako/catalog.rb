# frozen_string_literal: true

module Kobako
  # Kobako::Catalog — Sandbox-level configuration and per-invocation
  # allocation tables. Houses the three host-side registries the Sandbox
  # owns: +Catalog::Namespaces+ (Namespace / Member registry),
  # +Catalog::Snippets+ (preloaded source / bytecode entries), and
  # +Catalog::Handler+ (per-invocation Handle ID allocator).
  #
  # See {SPEC.md Refinement → Internal Concepts}[link:../../SPEC.md] for
  # how Catalog fits alongside Transport and Runtime.
  module Catalog
  end
end
