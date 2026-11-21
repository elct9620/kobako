# frozen_string_literal: true

# Waiting for Ruby's collector, which decides when it frees an object nothing
# reaches. A test that leaves an object unreachable waits here rather than
# assume one collection frees it.
module Collector
  ROUNDS = 20

  # Of the WeakRefs +refs+, those still alive after ROUNDS full collections.
  def still_reachable(*refs)
    ROUNDS.times do
      break if refs.none?(&:weakref_alive?)

      GC.start(full_mark: true, immediate_sweep: true)
    end
    refs.select(&:weakref_alive?)
  end
end
