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

    # Every probe +gate:bench:smoke+ drives: the gated roster plus the
    # characterizations that also run against the default Guest Binary
    # in seconds. Derived from RELEASE_BENCHES so promoting a benchmark
    # into the gate never costs it the cheaper wiring check.
    SMOKE_BENCHES = (RELEASE_BENCHES + %w[
      preload_dispatch
      dispatch_glue
    ].map { |name| Paths.probe(name) }).uniq.freeze

    # Probes the smoke gate leaves out, each with the reason the gate
    # prints. A probe belongs here when smoking it would cost minutes or
    # depend on a build artifact +rake compile+ does not produce — the
    # gate runs on every default rake, so a slow or artifact-dependent
    # check is one that gets worked around.
    SMOKE_EXCLUSIONS = {
      "memory" => "samples RSS across 10 000 invocations; the workload is fixed in the probe",
      "concurrent/threads" => "wall-clock across Thread counts; the workload is fixed in the probe",
      "concurrent/gvl_scheduling" => "wall-clock hold-vs-release sweep; the workload is fixed in the probe",
      "regexp" => "drives the kobako+regexp-unicode variant, which rake compile does not build"
    }.freeze
  end
end
