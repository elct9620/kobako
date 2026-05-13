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
        def write(value)
          check_encodable!(value)
          @buffer << Factory.instance.dump(value)
          self
        rescue ::RangeError => e
          # +RangeError+ surfaces from the gem when an Integer falls outside
          # msgpack's i64..u64 representable range. SPEC's int family covers
          # exactly that range; anything wider is a wire violation.
          raise UnsupportedType, e.message
        end

        private

        # SPEC's type-mapping table is a closed set: anything outside the 10
        # supported Ruby idioms (nil / Boolean / Integer / Float / String /
        # Array / Hash / +Kobako::Wire::Handle+ / +Kobako::Wire::Exception+)
        # is a wire violation. The msgpack gem will silently encode Symbols as
        # bin (or, with newer defaults, ext 0x00) and raise +NoMethodError+
        # for arbitrary objects; we want a single, well-typed
        # +UnsupportedType+ in both cases.
        def check_encodable!(value)
          case value
          when nil, true, false, Integer, Float, String, Array, Hash, Handle, Exception
            recurse_check(value)
          else
            raise UnsupportedType, "no wire encoding for #{value.class}: #{value.inspect}"
          end
        end

        def recurse_check(value)
          case value
          when Array
            value.each { |v| check_encodable!(v) }
          when Hash
            value.each do |k, v|
              check_encodable!(k)
              check_encodable!(v)
            end
          end
        end
      end
    end
  end
end
