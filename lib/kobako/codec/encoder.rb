# frozen_string_literal: true

require "msgpack"

require_relative "error"
require_relative "ext_types"

module Kobako
  module Codec
    # Module-level entry point for the host side of the kobako wire
    # ({docs/wire-codec.md}[link:../../../docs/wire-codec.md] § Type Mapping).
    #
    # The codec backbone is the official +msgpack+ gem: integers, floats,
    # strings, arrays, and maps go through the gem's narrowest-encoding
    # logic; the three kobako-specific ext types (0x00 Symbol, 0x01
    # Capability Handle, 0x02 Exception envelope) are registered by
    # ExtTypes on the process-wide factory.
    #
    # Public API is a single function — +.encode+. The codec is stateless;
    # there is no buffer accumulator and no streaming write API. Callers
    # that need to concatenate multiple encodings build the bytes
    # themselves.
    module Encoder
      # Encode +value+ to wire bytes (binary-encoded String).
      # SPEC's 12-entry type mapping is a closed set: a value outside it is
      # rejected as +UnsupportedType+ by the factory's +BasicObject+ guard
      # (ExtTypes#register_unrepresentable), which raises before the msgpack
      # gem can route the value through +to_msgpack+ — so a permissive
      # +method_missing+ object cannot answer that probe and mis-encode. The
      # rescue below maps the two violations the guard does not reach onto the
      # same error: an integer outside i64..u64 (+RangeError+) and any
      # packer-internal +NoMethodError+.
      def self.encode(value)
        FACTORY.dump(value)
      rescue ::RangeError, ::NoMethodError => e
        raise UnsupportedType, e.message
      end
    end
  end
end
