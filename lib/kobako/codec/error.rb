# frozen_string_literal: true

module Kobako
  module Codec
    class Error < StandardError; end # :nodoc:
    class TruncatedInputError < Error; end # :nodoc:

    # Also raised on encode for a value nested past what the packer can walk.
    class InvalidTypeError < Error; end # :nodoc:

    class InvalidEncodingError < Error; end # :nodoc:
    class UnsupportedTypeError < Error; end # :nodoc:
  end
end
