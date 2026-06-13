# frozen_string_literal: true

require_relative "capture"
require_relative "usage"

module Kobako
  # Kobako::Snapshot — per-invocation observable bundle returned from
  # +Kobako::Runtime#eval+ and +#run+.
  #
  # The magnus class (see ext/kobako/src/snapshot.rs) carries seven raw
  # readers: +return_bytes+, +stdout_bytes+, +stdout_truncated+,
  # +stderr_bytes+, +stderr_truncated+, +wall_time+, +memory_peak+. This
  # file reopens the class to add the Ruby-side helpers that pack those
  # raw fields into the user-facing value objects +Kobako::Capture+ and
  # +Kobako::Usage+ — the same shape +Kobako::Sandbox+ exposes to the
  # Host App.
  class Snapshot
    # Wrap the stdout capture pair (+stdout_bytes+, +stdout_truncated+)
    # as a +Kobako::Capture+ value object. The byte content never carries
    # a truncation sentinel; +#truncated?+ is the only way to observe
    # that the cap was hit.
    def stdout
      Capture.new(bytes: stdout_bytes, truncated: stdout_truncated)
    end

    # Wrap the stderr capture pair as a +Kobako::Capture+ value object.
    # Mirror of +#stdout+.
    def stderr
      Capture.new(bytes: stderr_bytes, truncated: stderr_truncated)
    end

    # Wrap the per-last-invocation usage pair (+wall_time+,
    # +memory_peak+) as a +Kobako::Usage+ value object.
    def usage
      Usage.new(wall_time: wall_time, memory_peak: memory_peak)
    end
  end
end
