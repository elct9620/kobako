# frozen_string_literal: true

module Kobako
  # Per-last-invocation resource accounting for a +Kobako::Sandbox+
  # ({docs/behavior.md B-35}[link:../../docs/behavior.md]). Carries two
  # readers populated by every +#eval+ / +#run+ invocation:
  #
  #   * +wall_time+ — the Float number of seconds the guest export call
  #     spent inside wasmtime during the most recent invocation. The
  #     measurement bracket aligns with the +timeout+ deadline
  #     ({docs/behavior.md B-01}[link:../../docs/behavior.md]); time spent
  #     in host Service callbacks is included, but everything that runs
  #     after the guest export returns — the post-export
  #     +OUTCOME_BUFFER+ fetch and decode, plus stdout / stderr capture
  #     readout — is excluded.
  #   * +memory_peak+ — the Integer high-water mark, in bytes, of the
  #     per-invocation +memory.grow+ delta past the linear-memory size
  #     captured at invocation entry. Same baseline accounting as
  #     +memory_limit+ ({docs/behavior.md E-20}[link:../../docs/behavior.md]):
  #     the mruby image's initial allocation and any prior-invocation
  #     watermark sit outside the measurement. On +MemoryLimitError+
  #     +memory_peak+ never exceeds the configured cap because the
  #     rejected +desired+ value is not promoted into the high-water.
  #
  # Both readers are populated on every outcome, including +TrapError+
  # branches, so the Host App can read +Sandbox#usage+ after rescuing a
  # trap to diagnose how much of the budget the failing invocation
  # consumed. Before the first invocation +Sandbox#usage+ returns the
  # pre-invocation sentinel +Kobako::Usage::EMPTY+.
  #
  # Built on the +class X < Data.define(...)+ subclass form (the
  # Steep-friendly shape — see +lib/kobako/outcome/panic.rb+).
  class Usage < Data.define(:wall_time, :memory_peak)
    # Pre-invocation sentinel ({docs/behavior.md B-35}[link:../../docs/behavior.md]).
    # Reused by +Sandbox+ before any invocation has run so callers do not
    # need to handle a +nil+ +#usage+.
    EMPTY = new(wall_time: 0.0, memory_peak: 0)
  end
end
