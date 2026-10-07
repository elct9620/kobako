# frozen_string_literal: true

require_relative "error"

module Kobako
  module Codec
    module Utils # :nodoc:
      def self.assert_utf8!(string, label)
        return if string.valid_encoding?

        raise InvalidEncodingError, "#{label} is not valid UTF-8"
      end

      # Only for a value object built outside a Decoder.decode block, whose
      # rescue already maps the same way; a host-layer +ArgumentError+
      # elsewhere should propagate unchanged.
      def self.with_boundary
        yield
      rescue ::ArgumentError => e
        raise InvalidTypeError, e.message
      end
    end
  end
end
