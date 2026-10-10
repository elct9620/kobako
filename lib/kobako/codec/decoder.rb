# frozen_string_literal: true

require "msgpack"

require_relative "error"
require_relative "ext_types"
require_relative "utils"

module Kobako
  module Codec
    module Decoder # :nodoc:
      # The block runs inside this method's rescue, so a Value Object built
      # from the decoded payload reports a broken invariant as
      # InvalidTypeError without its own Utils.with_boundary.
      def self.decode(bytes)
        value = FACTORY.load(bytes.b)
        validate_utf8!(value)
        block_given? ? yield(value) : value
      # msgpack gem raises the format/type errors below; +ArgumentError+
      # covers a yielded block's Value Object invariants — a wire violation
      # too, so it maps to InvalidTypeError.
      rescue ::MessagePack::UnknownExtTypeError, ::MessagePack::MalformedFormatError,
             ::MessagePack::StackError, ::ArgumentError => e
        raise InvalidTypeError, e.message
      # +UnpackError+ is the gem's umbrella class for short-read /
      # incomplete-buffer faults; +EOFError+ covers underflow at the
      # buffer edge.
      rescue ::MessagePack::UnpackError, ::EOFError => e
        raise TruncatedInputError, e.message
      end

      # +str+ family payloads must be UTF-8
      # (docs/wire/payload-msgpack.md § Text and Bytes). The msgpack gem
      # returns UTF-8-tagged Strings for
      # str family but does not validate the bytes; +bin+ family decodes
      # to ASCII-8BIT. Walk the tree once and reject invalid UTF-8 in any
      # str-typed leaf via Utils.assert_utf8!.
      class << self
        private

        def validate_utf8!(value)
          case value
          when String then Utils.assert_utf8!(value, "str payload") if value.encoding == Encoding::UTF_8
          when Array  then value.each { |v| validate_utf8!(v) }
          when Hash
            value.each do |key, val|
              validate_utf8!(key)
              validate_utf8!(val)
            end
          end
        end
      end
    end
  end
end
