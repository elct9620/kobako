# frozen_string_literal: true

# Regression benchmark support module
# ===================================
#
# Pure-Ruby data backing +tasks/benchmark.rake+. Owns the SPEC.md
# #1..#5 benchmark roster that +rake bench+ runs as the release gate.

# Release-gate benchmark roster. See sibling +tasks/benchmark.rake+ for
# the rake DSL.
module KobakoBench
  RELEASE_BENCHES = %w[
    benchmark/cold_start.rb
    benchmark/rpc_roundtrip.rb
    benchmark/codec.rb
    benchmark/mruby_eval.rb
    benchmark/handle_table.rb
  ].freeze
end
