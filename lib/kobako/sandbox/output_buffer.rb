# frozen_string_literal: true

module Kobako
  class Sandbox
    # In-memory bounded byte buffer for one of the guest's output channels.
    # Tracks accumulated bytes (binary-encoded) and enforces the per-channel
    # cap by truncating-with-marker ({SPEC.md B-04}[link:../../../SPEC.md]).
    #
    # When the accumulated byte count would exceed the limit, the buffer keeps
    # as many leading bytes as fit and seals itself. Subsequent appends are
    # discarded. On the next read, +OUTPUT_TRUNCATION_MARKER+ is appended to
    # signal the overflow to the caller.
    class OutputBuffer
      # Marker appended to a buffer that hit its capture limit
      # ({SPEC.md B-04}[link:../../../SPEC.md]).
      OUTPUT_TRUNCATION_MARKER = "[truncated]"

      attr_reader :limit

      def initialize(limit)
        raise ArgumentError, "limit must be a positive Integer" unless limit.is_a?(Integer) && limit.positive?

        @limit = limit
        @bytes = String.new(encoding: Encoding::ASCII_8BIT)
        @truncated = false
      end

      # Append +bytes+ to the buffer. If the append would push the
      # cumulative byte count past the limit, the buffer keeps as many
      # leading bytes as fit and seals itself; subsequent appends are
      # discarded. {SPEC.md B-04}[link:../../../SPEC.md] — truncation is a
      # non-error outcome.
      def <<(bytes)
        return self if @truncated

        appended = bytes.to_s.b
        room = @limit - @bytes.bytesize
        if appended.bytesize <= room
          @bytes << appended
        else
          @bytes << appended.byteslice(0, room) if room.positive?
          @truncated = true
        end
        self
      end

      # Returns +true+ when the buffer was sealed by an overflow.
      def truncated?
        @truncated
      end

      # Returns the number of bytes currently stored.
      def bytesize
        @bytes.bytesize
      end

      # Returns +true+ when the buffer is empty.
      def empty?
        @bytes.empty?
      end

      # Returns the accumulated bytes as a UTF-8 String, with the
      # +[truncated]+ marker appended when the buffer overflowed.
      def to_s
        copy = @bytes.dup
        copy << OUTPUT_TRUNCATION_MARKER.b if @truncated
        copy.force_encoding(Encoding::UTF_8)
        copy.valid_encoding? ? copy : copy.dup.force_encoding(Encoding::ASCII_8BIT)
      end

      # Reset the buffer to empty. Used at the per-+#run+ boundary.
      def clear
        @bytes.clear
        @truncated = false
        self
      end
    end
  end
end
