# frozen_string_literal: true

module Kobako
  # What one invocation wrote to stdout or stderr, up to that channel's
  # cap, together with whether it wrote past the cap. Frozen, so the bytes
  # and the flag always travel together.
  class Capture
    attr_reader :bytes

    # The bytes read as UTF-8 when they are valid UTF-8, and as binary
    # otherwise, so output that is not text stays inspectable instead of
    # raising. The caller's String is copied, never changed.
    def initialize(bytes:, truncated:)
      copy = bytes.dup.force_encoding(Encoding::UTF_8)
      copy.force_encoding(Encoding::ASCII_8BIT) unless copy.valid_encoding?
      @bytes = copy
      @truncated = truncated
      freeze
    end

    # Whether the invocation wrote past this channel's cap.
    def truncated? = @truncated

    # The capture before any invocation has written anything.
    EMPTY = new(bytes: "", truncated: false)
  end
end
