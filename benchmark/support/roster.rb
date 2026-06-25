# frozen_string_literal: true

require_relative "paths"

module Kobako
  module Bench
    # Release-gate benchmark roster — the SPEC.md #1..#6 probes +rake
    # bench+ runs as the gate, resolved to absolute probe paths. See
    # +tasks/bench/+ for the rake DSL that drives them.
    RELEASE_BENCHES = %w[
      cold_start
      transport_roundtrip
      codec
      mruby_eval
      catalog_handles
      yield_roundtrip
    ].map { |name| Paths.probe(name) }.freeze
  end
end
