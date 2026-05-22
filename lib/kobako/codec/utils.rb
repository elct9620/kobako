# frozen_string_literal: true

require_relative "error"
require_relative "../handle"

module Kobako
  module Codec
    # Wire-codec helpers shared by the host-side encoders and decoders.
    # Three concerns live here today:
    #
    #   - UTF-8 assertion at the wire boundary
    #     ({docs/wire-codec.md}[link:../../../docs/wire-codec.md]
    #     § str/bin Encoding Rules and § Ext Types → ext 0x00). Used by
    #     {Decoder} when walking +str+ family payloads and by {Factory}
    #     when validating the +ext 0x00+ Symbol payload.
    #   - Wire-boundary +ArgumentError+ translation
    #     ({wire_boundary}) so the public taxonomy stays
    #     {Kobako::Codec::Error}.
    #   - Wire-representability predicate ({wire_representable?}) and
    #     the symmetric host→guest +#run+ argument walk
    #     ({deep_wrap}) used by +Kobako::Invocation#encode+ to route
    #     non-wire-representable leaves through the Sandbox's
    #     +Kobako::HandleTable+
    #     ({docs/behavior.md B-34}[link:../../../docs/behavior.md]).
    #
    # All helpers are pure — they only inspect inputs, never mutate
    # them — except {deep_wrap}, whose only side effect is allocating
    # new Handle ids into the supplied table.
    module Utils
      module_function

      # Raise {InvalidEncoding} unless +string+'s bytes are valid under
      # its current encoding tag. +label+ is the caller-supplied prefix
      # for the error message (e.g. +"str payload"+, +"ext 0x00 payload"+).
      def assert_utf8!(string, label)
        return if string.valid_encoding?

        raise InvalidEncoding, "#{label} is not valid UTF-8"
      end

      # Run +block+ at the wire boundary: every wire Value Object
      # (Handle / Fault / Request / Response / Panic) raises
      # +ArgumentError+ when an invariant is violated at construction,
      # and the wire boundary surfaces those violations to callers as
      # {InvalidType} so the public taxonomy stays
      # {Kobako::Codec::Error} and never leaks +ArgumentError+ from the
      # Ruby standard library.
      #
      # Wrap any block that constructs a wire Value Object from decoded
      # bytes with this helper to keep the five decode sites uniform —
      # Request / Response in +Kobako::RPC+, Panic map in
      # +Kobako::Outcome+, and the Handle / Fault ext-type unpackers in
      # {Factory}. Do not use it for general-purpose validation outside
      # the wire boundary — host-layer +ArgumentError+ values should
      # propagate unchanged.
      def wire_boundary
        yield
      rescue ::ArgumentError => e
        raise InvalidType, e.message
      end

      # Inclusive Integer range the msgpack gem encodes without raising
      # +RangeError+ at encode time — signed +int 64+ minimum through
      # unsigned +uint 64+ maximum
      # ({docs/wire-codec.md}[link:../../../docs/wire-codec.md] § Type
      # Mapping #3, the +fixint+ / +int 8..64+ / +uint 8..64+ union).
      # Anchored as a +Range+ so {primitive_wire_type?} stays a single
      # dispatch line. This is the codec's wire-encode domain — not to
      # be confused with the Handle id range, which lives on
      # +Kobako::Handle+ as +MIN_ID+ / +MAX_ID+ (1..2^31 − 1) and
      # represents a different concept entirely.
      MSGPACK_INT_RANGE = (-(2**63)..((2**64) - 1))

      # Wire-type predicate
      # ({docs/wire-codec.md}[link:../../../docs/wire-codec.md] § Type
      # Mapping). Returns +true+ when +value+ belongs to the closed
      # 12-entry wire set — +nil+, +TrueClass+, +FalseClass+, +Integer+
      # (in the +i64..u64+ value domain), +Float+, +String+, +Symbol+,
      # +Kobako::Handle+, +Array+ whose every element is itself
      # wire-representable, or +Hash+ whose every key and value are
      # wire-representable. Integers outside the codec's signed-64 /
      # unsigned-64 union are rejected so the predicate agrees with the
      # msgpack gem's encode-time +RangeError+ behaviour the codec
      # already surfaces as {UnsupportedType}.
      def wire_representable?(value)
        primitive_wire_type?(value) || container_wire_representable?(value)
      end

      # Deep-walk Array / Hash containers in +value+ and replace every
      # leaf that fails {wire_representable?} with a +Kobako::Handle+
      # allocated from +handle_table+
      # ({docs/behavior.md B-34}[link:../../../docs/behavior.md]). The
      # walk only descends through wire-representable container shapes
      # (Array, Hash) one structural level at a time; a non-
      # wire-representable leaf is wrapped as-is without inspecting its
      # internal structure. An existing +Kobako::Handle+ is wire-
      # representable and passes through unchanged — auto-wrap never
      # re-wraps a Handle.
      #
      # +value+ may be any Ruby value; +handle_table+ must respond to
      # +#alloc(object) -> Integer+ (a host-side
      # +Kobako::HandleTable+). Returns a structurally equivalent value
      # whose leaves are either wire-representable or +Kobako::Handle+
      # tokens.
      def deep_wrap(value, handle_table)
        case value
        when ::Array then value.map { |element| Utils.deep_wrap(element, handle_table) }
        when ::Hash  then value.transform_values { |val| Utils.deep_wrap(val, handle_table) }
        else
          wire_representable?(value) ? value : Kobako::Handle.new(handle_table.alloc(value))
        end
      end

      # Predicate split out of {wire_representable?} for cyclomatic
      # budget — the closed-set non-container branch. Returns +true+ for
      # the wire scalar leaves and an existing Handle.
      def primitive_wire_type?(value)
        case value
        when ::NilClass, ::TrueClass, ::FalseClass, ::Float, ::String, ::Symbol, Kobako::Handle then true
        when ::Integer then MSGPACK_INT_RANGE.cover?(value)
        else false
        end
      end

      # Predicate split out of {wire_representable?} for cyclomatic
      # budget — the container branch. Recurses into Array elements and
      # Hash key+value pairs through the public {wire_representable?}.
      def container_wire_representable?(value)
        case value
        when ::Array then value.all? { |element| Utils.wire_representable?(element) }
        when ::Hash  then value.all? { |key, val| Utils.wire_representable?(key) && Utils.wire_representable?(val) }
        else false
        end
      end
    end
  end
end
