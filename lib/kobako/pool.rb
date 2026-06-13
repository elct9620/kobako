# frozen_string_literal: true

require_relative "errors"
require_relative "sandbox"

module Kobako
  # Kobako::Pool — a bounded set of warm, identically set-up Sandboxes
  # handed out one exclusive holder at a time.
  #
  # Construction forwards every +Kobako::Sandbox.new+ keyword verbatim
  # and holds the optional block as the per-Sandbox setup hook; a
  # checkout prefers an idle Sandbox and constructs a new one only when
  # none is idle and fewer than +slots+ exist. +#with+ blocks up
  # to +checkout_timeout+ seconds when every slot is held, applies
  # the +TrapError+ discard-and-recreate contract at checkin, and
  # the Pool releases everything with its own reachability — there is no
  # teardown verb.
  class Pool
    # The +#with+ wait bound applied when +checkout_timeout+ is not given.
    DEFAULT_CHECKOUT_TIMEOUT_SECONDS = 5.0

    # Build a Pool of up to +slots+ Sandboxes. +slots+ is
    # a positive Integer; +checkout_timeout+ bounds the +#with+ wait in
    # seconds (+nil+ waits indefinitely); every other keyword is
    # forwarded verbatim to +Kobako::Sandbox.new+. The optional block
    # runs exactly once per constructed Sandbox — it is the setup window
    # for +#define+ / +#preload+ before that Sandbox's first checkout.
    # No Sandbox is constructed here. Raises +ArgumentError+ for an
    # invalid +slots+ / +checkout_timeout+.
    def initialize(slots:, checkout_timeout: DEFAULT_CHECKOUT_TIMEOUT_SECONDS, **sandbox_options, &setup)
      validate_slots!(slots)
      @slots = slots
      @checkout_timeout = normalize_checkout_timeout(checkout_timeout)
      @sandbox_options = sandbox_options
      @setup = setup
      @idle = [] # : Array[Kobako::Sandbox]
      @constructed = 0
      @mutex = Mutex.new
      @slot_freed = ConditionVariable.new
    end

    # Yield one exclusively-held Sandbox to the block and return the
    # block's value. Blocks while every slot is held; raises
    # +Kobako::PoolTimeoutError+ once the wait exceeds +checkout_timeout+.
    # The Sandbox returns to the pool at block exit — unless the block raised
    # +Kobako::TrapError+, in which case the unrecoverable Sandbox is
    # discarded and its slot refills by a fresh construction on next
    # demand.
    def with
      sandbox = checkout
      begin
        yield sandbox
      rescue TrapError
        release_capacity!
        sandbox = nil
        raise
      ensure
        checkin(sandbox) if sandbox
      end
    end

    private

    # Acquire a Sandbox and hand it over in pre-invocation state — empty
    # output buffers and truncation predicates false.
    def checkout
      acquire.tap(&:reset_invocation_state!)
    end

    # The idle-first claim loop: an idle Sandbox wins, unclaimed
    # capacity constructs, and a full pool waits for a checkin.
    def acquire
      timeout = @checkout_timeout
      deadline = timeout && (monotonic_now + timeout)
      loop do
        action, sandbox = claim_or_wait(deadline)
        return sandbox if action == :idle && sandbox
        return construct_slot if action == :build
      end
    end

    # Single locked decision point for one claim attempt. Waiting
    # happens inside the lock (so a checkin can wake it); construction
    # happens outside (so a slow setup block never holds the lock) —
    # capacity is reserved here and released by +construct_slot+ on
    # failure.
    def claim_or_wait(deadline)
      @mutex.synchronize do
        return [:idle, @idle.pop] unless @idle.empty?

        if @constructed < @slots
          @constructed += 1
          return [:build, nil]
        end

        await_slot!(deadline)
        [:retry, nil]
      end
    end

    # Wait for a checkin or freed capacity; raises
    # +Kobako::PoolTimeoutError+ once +deadline+ has passed. Must
    # run while holding +@mutex+.
    def await_slot!(deadline)
      remaining = deadline && (deadline - monotonic_now)
      if remaining && remaining <= 0
        raise PoolTimeoutError,
              "no Sandbox returned within #{@checkout_timeout}s: all #{@slots} slots are held"
      end

      @slot_freed.wait(@mutex, remaining)
    end

    # Construct and set up one pooled Sandbox against the capacity
    # reserved by +claim_or_wait+. Construction and setup-block errors
    # propagate to the checkout caller unchanged; the reserved
    # capacity is released so a later checkout can retry.
    def construct_slot
      done = false
      sandbox = Sandbox.new(**@sandbox_options)
      @setup&.call(sandbox)
      done = true
      sandbox
    ensure
      release_capacity! unless done
    end

    # Return a Sandbox to the idle list and wake one waiting checkout.
    def checkin(sandbox)
      @mutex.synchronize do
        @idle.push(sandbox)
        @slot_freed.signal
      end
    end

    # Give back reserved-but-unfilled capacity — a failed construction or
    # a discarded Sandbox — and wake one waiting checkout to claim it.
    def release_capacity!
      @mutex.synchronize do
        @constructed -= 1
        @slot_freed.signal
      end
    end

    # The wait deadline runs on the monotonic clock so a wall-clock jump
    # cannot stretch or cut the checkout wait.
    def monotonic_now
      Process.clock_gettime(Process::CLOCK_MONOTONIC)
    end

    # Pre-flight for +slots+ — no coercion, a positive Integer is
    # the only accepted shape.
    def validate_slots!(slots)
      return if slots.is_a?(Integer) && slots.positive?

      raise ArgumentError, "slots must be a positive Integer, got #{slots.inspect}"
    end

    # Coerce +checkout_timeout+ into the Float seconds the wait loop
    # consumes, or +nil+ to wait indefinitely — the same normalisation
    # idiom +SandboxOptions+ applies to +timeout+.
    def normalize_checkout_timeout(checkout_timeout)
      return nil if checkout_timeout.nil?
      unless checkout_timeout.is_a?(Numeric)
        raise ArgumentError, "checkout_timeout must be Numeric or nil, got #{checkout_timeout.inspect}"
      end

      seconds = checkout_timeout.to_f
      unless seconds.positive? && seconds.finite?
        raise ArgumentError, "checkout_timeout must be > 0 and finite (got #{checkout_timeout})"
      end

      seconds
    end
  end
end
