# frozen_string_literal: true

# A meeting point for tests that must show several callers were inside
# the same code at once. Each caller waits, bounded, for the rest to
# arrive; callers serialized by the code under test can never all be
# present, so the first leaves alone once the bound passes.
class Rendezvous
  def initialize(parties, bound: 5)
    @parties = parties
    @bound = bound
    @arrived = 0
    @lock = Mutex.new
    @all_here = ConditionVariable.new
  end

  # Answers whether every party arrived before the bound passed.
  def meet
    @lock.synchronize do
      @arrived += 1
      @all_here.broadcast
      deadline = now + @bound
      @all_here.wait(@lock, deadline - now) while @arrived < @parties && now < deadline
      @arrived >= @parties
    end
  end

  private

  def now = Process.clock_gettime(Process::CLOCK_MONOTONIC)
end
