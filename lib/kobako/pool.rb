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

    # Build a Pool of up to +size+ Sandboxes, built on first demand with the
    # other keywords as Sandbox.new takes them. +checkout_timeout+ bounds how
    # many seconds #with waits, and +nil+ waits indefinitely. The block, if
    # given, sets up each Sandbox once, as with Sandbox#bind and
    # Sandbox#preload.
    #
    # Raises TypeError when +size+ is not an Integer or +checkout_timeout+
    # is not Numeric, and ArgumentError when either is not positive or
    # +checkout_timeout+ is not finite.
    def initialize(size:, checkout_timeout: DEFAULT_CHECKOUT_TIMEOUT_SECONDS, **sandbox_options, &setup)
      validate_size(size)
      @size = size
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
    #
    # An interrupt reaches the checkout only while it waits or builds, and
    # the block as it runs, as connection_pool's +with+ does, so none lands
    # between taking a Sandbox and handing it back.
    def with
      Thread.handle_interrupt(Exception => :never) do
        sandbox = acquire
        begin
          Thread.handle_interrupt(Exception => :immediate) { yield sandbox }
        ensure
          checkin(sandbox)
        end
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

        if @constructed < @size
          @constructed += 1
          return [:build, nil]
        end

        await_slot(deadline)
        [:retry, nil]
      end
    end

    # Must run while holding +@mutex+.
    def await_slot(deadline)
      remaining = deadline && (deadline - monotonic_now)
      if remaining && remaining <= 0
        raise PoolTimeoutError,
              "no Sandbox returned within #{@checkout_timeout}s: all #{@size} Sandboxes are held"
      end

      Thread.handle_interrupt(Exception => :immediate) { @slot_freed.wait(@mutex, remaining) }
    end

    # A failed construction releases its reserved capacity so a later
    # checkout can retry. Setup closes registration, so each holder gets
    # the Sandbox as the setup block left it.
    def construct_slot
      done = false
      sandbox = Thread.handle_interrupt(Exception => :immediate) { build_slot }
      done = true
      sandbox
    ensure
      release_capacity unless done
    end

    def build_slot
      sandbox = Sandbox.new(**@sandbox_options)
      @setup&.call(sandbox)
      sandbox.send(:seal, "on a pooled Sandbox once its setup ran")
      sandbox
    end

    def checkin(sandbox)
      @mutex.synchronize do
        @idle.push(sandbox)
        @slot_freed.broadcast
      end
    end

    def release_capacity
      @mutex.synchronize do
        @constructed -= 1
        @slot_freed.broadcast
      end
    end

    # The wait deadline runs on the monotonic clock so a wall-clock jump
    # cannot stretch or cut the checkout wait.
    def monotonic_now
      Process.clock_gettime(Process::CLOCK_MONOTONIC)
    end

    def validate_size(size)
      raise TypeError, "size must be an Integer, got #{size.class}" unless size.is_a?(Integer)
      return if size.positive?

      raise ArgumentError, "size must be a positive Integer, got #{size.inspect}"
    end

    def normalize_checkout_timeout(checkout_timeout)
      return nil if checkout_timeout.nil?
      unless checkout_timeout.is_a?(Numeric)
        raise TypeError, "checkout_timeout must be Numeric or nil, got #{checkout_timeout.inspect}"
      end

      seconds = checkout_timeout.to_f
      unless seconds.positive? && seconds.finite?
        raise ArgumentError, "checkout_timeout must be > 0 and finite (got #{checkout_timeout})"
      end

      seconds
    end
  end
end
