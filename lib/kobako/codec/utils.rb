# frozen_string_literal: true

require_relative "error"

module Kobako
  module Codec
    # Byte-boundary helpers shared by the host-side encoder and decoder.
    # Two concerns live here:
    #
    #   - UTF-8 assertion at the codec boundary
    #     ({docs/wire-codec.md}[link:../../../docs/wire-codec.md]
    #     § str/bin Encoding Rules and § Ext Types → ext 0x00). Used by
    #     Decoder when walking +str+ family payloads and by ExtTypes
    #     when validating the +ext 0x00+ Symbol payload.
    #   - +ArgumentError+ translation at the codec boundary
    #     (#with_boundary) so the public taxonomy stays
    #     Kobako::Codec::Error.
    #
    # Both helpers are pure — they only inspect inputs, never mutate them.
    # The host↔guest Handle substitution walk lives in HandleWalk.
    module Utils
      module_function

      # Raise InvalidEncoding unless +string+'s bytes are valid under
      # its current encoding tag. +label+ is the caller-supplied prefix
      # for the error message (e.g. +"str payload"+, +"Symbol payload"+).
      def assert_utf8!(string, label)
        return if string.valid_encoding?

        raise InvalidEncoding, "#{label} is not valid UTF-8"
      end

      # Run +block+ at the codec boundary: a value object raises
      # +ArgumentError+ when an invariant is violated at construction, and
      # this helper surfaces that as InvalidType so the public taxonomy
      # stays Kobako::Codec::Error and never leaks +ArgumentError+ from
      # the Ruby standard library.
      #
      # Reach for this only where a value object is constructed outside a
      # Decoder.decode block, whose rescue already performs the same
      # mapping (worked example: ExtTypes#unpack_handle building
      # +Handle.restore+ from a raw fixext payload). Do not use it for
      # general-purpose validation outside the codec boundary —
      # host-layer +ArgumentError+ values should propagate unchanged.
      def with_boundary
        yield
      rescue ::ArgumentError => e
        raise InvalidType, e.message
      end
    end
  end
end
