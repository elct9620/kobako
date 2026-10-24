# frozen_string_literal: true

module Kobako
  module Codec
    # Whether a Capability Handle crossed the decode in flight, and the
    # value budget a decode is held to, kept per thread so plain instance
    # variables stay sound.
    class State # :nodoc:
      # Thread-local slot holding the calling thread's State.
      STATE_KEY = :__kobako_codec_state__
      private_constant :STATE_KEY

      # The calling thread's State, built on first use so the mutable
      # state stays isolated to the thread that runs the codec call.
      def self.current
        Thread.current[STATE_KEY] ||= new
      end
      private_class_method :new

      def initialize
        @carried_handle = false
        @value_budget = nil
      end

      # The most values a decode on this thread may build, or nil for no
      # budget.
      attr_reader :value_budget

      # A nested invocation on the same thread restores the outer one's
      # budget when it returns.
      def within_value_budget(budget)
        outer = @value_budget
        @value_budget = budget
        yield
      ensure
        @value_budget = outer
      end

      # ExtTypes#unpack_handle is the sole chokepoint every Handle passes
      # through, so one decode pass records the whole tree.
      def track_handles
        @carried_handle = false
        result = yield
        [result, @carried_handle]
      end

      # Record that an ext 0x01 Capability Handle crossed the current
      # decode; #track_handles reports it to the bracketing caller.
      def record_handle
        @carried_handle = true
      end
    end

    private_constant :State
  end
end
