# frozen_string_literal: true

module Kobako
  module Codec
    # Codec-internal, per-thread state of the operation in flight: the
    # ext-envelope nesting depth and whether a Capability Handle crossed
    # the current decode. Thread scoping is what makes plain instance
    # variables sound — host codec calls run synchronously on their
    # owning thread, and a nested decode (an ext 0x02 Fault re-entering
    # through its +details+) reuses the same thread instance, so the
    # depth counter accumulates across the re-entry instead of resetting.
    class State
      # An ext 0x02 (Fault) envelope nests through its +details+ field, and
      # each level re-enters the codec with a fresh +MessagePack+ unpacker
      # whose built-in stack guard resets — so ext-envelope depth is tracked
      # here instead. The cap matches the wire's overall nesting bound and
      # keeps a nested chain from exhausting the native stack: an over-deep
      # chain fails as a clean wire error, never a stack-level trap.
      MAX_EXT_DEPTH = 128
      private_constant :MAX_EXT_DEPTH

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
        @ext_depth = 0
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

      # Track ext-envelope re-entry depth and refuse a chain past
      # MAX_EXT_DEPTH, raising +over_limit+ so the failure lands in the
      # caller's existing wire-error class. The next depth is checked before
      # it is committed, so an over-deep rejection leaves the counter
      # untouched, and the +ensure+ restores the entry value on the way out.
      def within_ext_frame(over_limit)
        depth = @ext_depth + 1
        raise over_limit, "ext envelope nesting exceeds #{MAX_EXT_DEPTH} levels" if depth > MAX_EXT_DEPTH

        @ext_depth = depth
        begin
          yield
        ensure
          @ext_depth = depth - 1
        end
      end
    end

    private_constant :State
  end
end
