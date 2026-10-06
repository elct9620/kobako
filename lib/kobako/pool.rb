# frozen_string_literal: true

require_relative "errors"
require_relative "sandbox"

module Kobako
  # A bounded set of warm, identically set-up Sandboxes, handed out to one
  # holder at a time. Every Sandbox it builds is kept: an invocation leaves
  # nothing behind, whatever ended it.
  class Pool
    # The default +checkout_timeout+: 5 seconds.
    DEFAULT_CHECKOUT_TIMEOUT_SECONDS = 5.0

    # Build a Pool of up to +slots+ Sandboxes, built on first demand with the
    # other keywords as Sandbox.new takes them. +checkout_timeout+ bounds how
    # many seconds #with waits, and +nil+ waits indefinitely. The block, if
    # given, sets up each Sandbox once, as with Sandbox#bind and
    # Sandbox#preload.
    #
    # Raises ArgumentError when +slots+ is not a positive Integer or
    # +checkout_timeout+ is not a positive finite number.
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
    # block's value; the Sandbox returns to the pool however the block
    # exits. Raises PoolTimeoutError once every slot has stayed held past
    # +checkout_timeout+.
    def with
      sandbox = acquire
      begin
        yield sandbox
      ensure
        checkin(sandbox)
      end
    end

    private

    def acquire
      timeout = @checkout_timeout
      deadline = timeout && (monotonic_now + timeout)
      loop do
        action, sandbox = claim_or_wait(deadline)
        return sandbox if action == :idle && sandbox
        return construct_slot if action == :build
      end
    end

    # Waiting happens inside the lock (so a checkin can wake it);
    # construction happens outside (so a slow setup block never holds the
    # lock) — capacity is reserved here and released by +construct_slot+
    # on failure.
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

    # Must run while holding +@mutex+.
    def await_slot!(deadline)
      remaining = deadline && (deadline - monotonic_now)
      if remaining && remaining <= 0
        raise PoolTimeoutError,
              "no Sandbox returned within #{@checkout_timeout}s: all #{@slots} slots are held"
      end

      @slot_freed.wait(@mutex, remaining)
    end

    # A failed construction releases its reserved capacity so a later
    # checkout can retry.
    def construct_slot
      done = false
      sandbox = Sandbox.new(**@sandbox_options)
      @setup&.call(sandbox)
      done = true
      sandbox
    ensure
      release_capacity! unless done
    end

    def checkin(sandbox)
      @mutex.synchronize do
        @idle.push(sandbox)
        @slot_freed.signal
      end
    end

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

    def validate_slots!(slots)
      return if slots.is_a?(Integer) && slots.positive?

      raise ArgumentError, "slots must be a positive Integer, got #{slots.inspect}"
    end

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
