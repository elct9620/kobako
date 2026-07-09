# frozen_string_literal: true

require "forwardable"

require_relative "ext_types"

module Kobako
  module Codec
    # Per-thread carrier of the codec's per-operation state: the
    # ext-envelope nesting depth and whether a Capability Handle crossed
    # the most recent decode. Byte work happens on the process-wide
    # registered factory (see ExtTypes.build_factory); this class only
    # scopes the mutable state to one thread, which is sound because host
    # codec calls run synchronously on their owning thread and a nested
    # decode (an ext 0x02 Fault re-entering through its +details+) reuses
    # the same thread instance, so the depth counter accumulates across
    # the re-entry instead of resetting.
    #
    # Class-level +Factory.dump+ / +Factory.load+ /
    # +Factory.reset_handle_tracking!+ / +Factory.saw_handle?+ resolve to
    # the calling thread's instance via +SingleForwardable+.
    class Factory
      extend SingleForwardable

      # An ext 0x02 (Fault) envelope nests through its +details+ field, and
      # each level re-enters the codec with a fresh +MessagePack+ unpacker
      # whose built-in stack guard resets — so ext-envelope depth is tracked
      # on the instance instead. The cap matches the wire's overall nesting
      # bound and keeps a nested chain from exhausting the native stack: an
      # over-deep chain fails as a clean wire error, never a stack-level trap.
      MAX_EXT_DEPTH = 128
      private_constant :MAX_EXT_DEPTH

      # Thread-local slot holding the calling thread's cached Factory.
      FACTORY_KEY = :__kobako_codec_factory__
      private_constant :FACTORY_KEY

      # The calling thread's cached Factory, built on first use so the
      # per-operation state stays isolated to the thread that runs the
      # codec call.
      def self.instance
        Thread.current[FACTORY_KEY] ||= new
      end
      private_class_method :new

      # Class-level shortcuts so callers can write +Factory.dump(v)+ instead
      # of +Factory.instance.dump(v)+; each resolves to the calling thread's
      # instance.
      def_single_delegators :instance, :dump, :load, :reset_handle_tracking!, :saw_handle?

      def initialize
        @ext_depth = 0
        @saw_handle = false
      end

      # Encode +value+ to wire bytes through the process-wide registered
      # factory.
      def dump(value)
        FACTORY.dump(value)
      end

      # Decode wire +bytes+ through the process-wide registered factory.
      def load(bytes)
        FACTORY.load(bytes)
      end

      # Clear the decode-time Handle signal ahead of a decode; pair with
      # #saw_handle? read straight after to learn whether the tree carried a
      # Capability Handle. ExtTypes#unpack_handle is the sole chokepoint
      # every Handle passes through, so it records the whole tree in one
      # decode pass and a caller can then skip an all-identity Handle walk
      # when none was present.
      def reset_handle_tracking!
        @saw_handle = false
      end

      # Whether the most recent decode on this thread carried an ext 0x01
      # Handle. Only meaningful immediately after a decode bracketed by
      # #reset_handle_tracking!.
      def saw_handle?
        @saw_handle
      end

      # Record that an ext 0x01 Capability Handle crossed the current
      # decode; read back through #saw_handle?.
      def record_handle!
        @saw_handle = true
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
  end
end
