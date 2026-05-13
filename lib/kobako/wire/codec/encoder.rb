# frozen_string_literal: true

require "msgpack"

require_relative "error"
require_relative "../handle"
require_relative "../exception"
require_relative "factory"

module Kobako
  module Wire
    module Codec
      # Thin wrapper around +MessagePack::Factory+ for the host side of the
      # kobako wire (SPEC.md → Wire Codec → Type Mapping).
      #
      # The codec backbone is the official +msgpack+ gem: integers, floats,
      # strings, arrays, and maps go through the gem's narrowest-encoding
      # logic; the two kobako-specific ext types (0x01 Capability Handle and
      # 0x02 Exception envelope) are registered on +Factory+ via
      # {Kobako::Wire::Codec::Factory.instance}.
      #
      # The class still exists as a public API surface so callers do not need
      # to know the codec backend — the previous hand-rolled implementation
      # exposed +Encoder.encode(value)+ / +Encoder.new(buf).write(value)+ and
      # those entry points are preserved.
      class Encoder
        # Single-shot helper.
        def self.encode(value)
          new.tap { |enc| enc.write(value) }.buffer
        end

        def initialize(buffer = String.new(encoding: Encoding::ASCII_8BIT))
          @buffer = buffer
        end

        attr_reader :buffer

        # Encode +value+ and append the resulting bytes to the backing buffer.
        # Wire violations surface as +UnsupportedType+: SPEC's 10-entry type
        # mapping is a closed set, and anything outside it is rejected by
        # either the Factory (Symbol — see +register_symbol_rejection+) or
        # the msgpack gem itself (arbitrary objects raise +NoMethodError+
        # from missing +to_msgpack+, integers outside i64..u64 raise
        # +RangeError+).
        def write(value)
          @buffer << Factory.instance.dump(value)
          self
        rescue ::RangeError, ::NoMethodError => e
          raise UnsupportedType, e.message
        end
      end
    end
  end
end
