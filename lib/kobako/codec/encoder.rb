# frozen_string_literal: true

require "msgpack"

require_relative "error"
require_relative "../wire/handle"
require_relative "../wire/exception"
require_relative "factory"

module Kobako
  module Codec
    # Module-level entry point for the host side of the kobako wire
    # (SPEC.md → Wire Codec → Type Mapping).
    #
    # The codec backbone is the official +msgpack+ gem: integers, floats,
    # strings, arrays, and maps go through the gem's narrowest-encoding
    # logic; the three kobako-specific ext types (0x00 Symbol, 0x01
    # Capability Handle, 0x02 Exception envelope) are registered on
    # +Factory+ via {Kobako::Codec::Factory.instance}.
    #
    # Public API is a single function — {.encode}. The codec is stateless;
    # there is no buffer accumulator and no streaming write API. Callers
    # that need to concatenate multiple encodings build the bytes
    # themselves (see +Kobako::Wire::Envelope+ for the canonical caller).
    module Encoder
      # Encode +value+ to wire bytes (binary-encoded String).
      # Wire violations surface as +UnsupportedType+: SPEC's 12-entry type
      # mapping is a closed set, and anything outside it is rejected by
      # the msgpack gem itself (arbitrary objects raise +NoMethodError+
      # from missing +to_msgpack+, integers outside i64..u64 raise
      # +RangeError+).
      def self.encode(value)
        Factory.instance.dump(value)
      rescue ::RangeError, ::NoMethodError => e
        raise UnsupportedType, e.message
      end
    end
  end
end
