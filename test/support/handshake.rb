# frozen_string_literal: true

# Waiting in a test for another thread to reach a point. The wait is never
# part of what passes: it only ends a test whose other side jammed, red, and
# says what never happened, which decides where to look next.
module Handshake
  # Far past any slowdown, so reaching it means the other side jammed.
  MAX_WAIT = 10

  # Wait until the block answers truthy, giving the other threads the CPU in
  # between; flunk with +never+ once MAX_WAIT seconds pass.
  def wait_until(never)
    ends = Process.clock_gettime(Process::CLOCK_MONOTONIC) + MAX_WAIT
    until yield
      flunk("#{never} within #{MAX_WAIT} s") if Process.clock_gettime(Process::CLOCK_MONOTONIC) > ends
      sleep 0.001
    end
  end
end
