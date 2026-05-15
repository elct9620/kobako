# frozen_string_literal: true

require_relative "error"

module Kobako
  module Codec
    # Wire-codec helpers shared by the host-side encoders and decoders.
    # The single concern today is UTF-8 assertion at the wire boundary
    # (SPEC.md → Wire Codec → str/bin Encoding Rules and Ext Types →
    # ext 0x00). Two call sites lean on this:
    #
    #   - {Decoder} validates +str+ family payloads as it walks the
    #     decoded value tree.
    #   - {Factory} validates the +ext 0x00+ Symbol payload after
    #     re-tagging the binary bytes as UTF-8.
    #
    # Encoding setup (re-tagging binary as UTF-8 when needed) stays at
    # the caller — only the assertion shape is shared. The helper does
    # not mutate +string+; it only inspects +String#valid_encoding?+
    # against +string+'s current encoding tag.
    module Utils
      module_function

      # Raise {InvalidEncoding} unless +string+'s bytes are valid under
      # its current encoding tag. +label+ is the caller-supplied prefix
      # for the error message (e.g. +"str payload"+, +"ext 0x00 payload"+).
      def assert_utf8!(string, label)
        return if string.valid_encoding?

        raise InvalidEncoding, "#{label} is not valid UTF-8"
      end

      # Wire-boundary translator: every wire Value Object (Handle /
      # Exception / Request / Response / Panic) raises +ArgumentError+
      # when an invariant is violated at construction. The wire boundary
      # surfaces those violations to callers as {InvalidType} so the
      # public taxonomy stays {Kobako::Codec::Error} and never leaks
      # +ArgumentError+ from the Ruby standard library.
      #
      # Wrap any block that constructs a wire Value Object from decoded
      # bytes with this helper to keep the five decode sites uniform —
      # Request / Response in +Kobako::RPC+, Panic map in
      # +Kobako::Outcome+, and the Handle / Exception ext-type unpackers
      # in {Factory}. Do not use it for general-purpose validation
      # outside the wire boundary — host-layer +ArgumentError+ values
      # should propagate unchanged.
      def translate_value_object_error
        yield
      rescue ::ArgumentError => e
        raise InvalidType, e.message
      end
    end
  end
end
