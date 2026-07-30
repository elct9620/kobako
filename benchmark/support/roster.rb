# frozen_string_literal: true

require_relative "paths"

module Kobako
  module Bench
    # Release-gate benchmark roster — the probes SPEC.md's Regression
    # benchmarks table names, resolved to absolute probe paths. See
    # +tasks/bench/+ for the rake DSL that drives them.
    RELEASE_BENCHES = %w[
      cold_start
      transport_roundtrip
      codec
      mruby_eval
      catalog_handles
      yield_roundtrip
      preload_dispatch
      dispatch_glue
      host_invocation
    ].map { |name| Paths.probe(name) }.freeze

    # The characterization suites +bench:all+ runs after the gated set,
    # named by their rake task. Kept out of the rake file so the roster
    # is one readable list and a suite cannot land in both this and the
    # gated set, where the second pass would overwrite the first's rows.
    SWEEP_TASKS = %w[
      concurrent
      guest_setup
      gvl_scheduling
      memory
      regexp
    ].freeze

    # The characterization probes cheap enough to smoke: each drives the
    # default Guest Binary and finishes in seconds, so the reason to leave
    # one out of the wiring check does not apply.
    SMOKE_CHARACTERIZATION = %w[guest_setup].freeze

    # Every probe +gate:bench:smoke+ drives. Derived from RELEASE_BENCHES
    # so promoting a benchmark into the gate never costs it the cheaper
    # wiring check.
    SMOKE_BENCHES = (RELEASE_BENCHES + SMOKE_CHARACTERIZATION.map { |name| Paths.probe(name) })
                    .uniq.freeze

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
