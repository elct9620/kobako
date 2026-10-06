# frozen_string_literal: true

module Kobako
  # What one invocation spent against its caps, measured the way the caps
  # measure it:
  #
  #   * +wall_time+ — Float seconds inside the guest, the span the
  #     +timeout+ deadline governs; Service callbacks count, reading the
  #     result and the captures afterwards does not.
  #   * +memory_peak+ — Integer bytes of memory the invocation grew,
  #     against the same baseline as +memory_limit+; it never exceeds the
  #     cap, even when the cap was hit.
  #
  # Filled on every outcome, traps included, so a Host App that rescues a
  # trap can read from the error's +#execution+ how much of the budget the
  # invocation used.
  class Usage < Data.define(:wall_time, :memory_peak)
    # The usage of a run that has not started: no time, no memory.
    EMPTY = new(wall_time: 0.0, memory_peak: 0)
  end
end
