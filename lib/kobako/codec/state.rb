# frozen_string_literal: true

module Kobako
  module Codec
    # Codec-internal, per-thread state of the operation in flight: whether
    # a Capability Handle crossed the current decode. Thread scoping is
    # what makes plain instance variables sound — host codec calls run
    # synchronously on their owning thread.
    class State
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
      end

      # Bracket a decode and return the block's result together with
      # whether the decoded tree carried an ext 0x01 Capability Handle.
      # ExtTypes#unpack_handle is the sole chokepoint every Handle passes
      # through, so one decode pass records the whole tree and a caller
      # can skip an all-identity Handle-resolution walk when none was
      # present.
      def track_handles
        @carried_handle = false
        result = yield
        [result, @carried_handle]
      end

      # Record that an ext 0x01 Capability Handle crossed the current
      # decode; #track_handles reports it to the bracketing caller.
      def record_handle!
        @carried_handle = true
      end
    end

    private_constant :State
  end
end
