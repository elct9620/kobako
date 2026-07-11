# frozen_string_literal: true

require_relative "catalog/handles"
require_relative "catalog/services"
require_relative "catalog/snippets"

module Kobako
  # Kobako::Catalog — Sandbox-level configuration and per-invocation
  # allocation tables. Houses the three host-side registries the Sandbox
  # owns: +Catalog::Services+ (path→Service binding registry),
  # +Catalog::Snippets+ (preloaded source / bytecode entries), and
  # +Catalog::Handles+ (per-invocation Handle ID allocator).
  #
  # See {SPEC.md Refinement → Internal Concepts}[link:../../SPEC.md] for
  # how Catalog fits alongside Transport and Runtime.
  module Catalog
  end
end
