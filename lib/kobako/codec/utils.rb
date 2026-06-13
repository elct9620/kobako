# frozen_string_literal: true

require_relative "error"
require_relative "../handle"

module Kobako
  module Codec
    # Codec helpers shared by the host-side encoders and decoders.
    # Three concerns live here today:
    #
    #   - UTF-8 assertion at the codec boundary
    #     ({docs/wire-codec.md}[link:../../../docs/wire-codec.md]
    #     § str/bin Encoding Rules and § Ext Types → ext 0x00). Used by
    #     {Decoder} when walking +str+ family payloads and by {Factory}
    #     when validating the +ext 0x00+ Symbol payload.
    #   - +ArgumentError+ translation at the codec boundary
    #     ({with_boundary}) so the public taxonomy stays
    #     {Kobako::Codec::Error}.
    #   - Representability predicate ({representable?}) and the symmetric
    #     host→guest +#run+ argument walk ({deep_wrap}) used by
    #     +Kobako::Transport::Run#encode+ to route non-representable leaves
    #     through the Sandbox's +Kobako::Catalog::Handles+.
    #
    # All helpers are pure — they only inspect inputs, never mutate
    # them — except {deep_wrap}, whose only side effect is allocating
    # new Handle ids into the supplied table.
    module Utils
      module_function

      # Raise {InvalidEncoding} unless +string+'s bytes are valid under
      # its current encoding tag. +label+ is the caller-supplied prefix
      # for the error message (e.g. +"str payload"+, +"Symbol payload"+).
      def assert_utf8!(string, label)
        return if string.valid_encoding?

        raise InvalidEncoding, "#{label} is not valid UTF-8"
      end

      # Run +block+ at the codec boundary: a value object raises
      # +ArgumentError+ when an invariant is violated at construction, and
      # this helper surfaces that as {InvalidType} so the public taxonomy
      # stays {Kobako::Codec::Error} and never leaks +ArgumentError+ from
      # the Ruby standard library.
      #
      # Reach for this only where a value object is constructed outside a
      # {Decoder.decode} block, whose rescue already performs the same
      # mapping (worked example: {Factory#unpack_handle} building
      # +Handle.restore+ from a raw fixext payload). Do not use it for
      # general-purpose validation outside the codec boundary —
      # host-layer +ArgumentError+ values should propagate unchanged.
      def with_boundary
        yield
      rescue ::ArgumentError => e
        raise InvalidType, e.message
      end

      # Inclusive Integer range the msgpack gem encodes without raising
      # +RangeError+ at encode time — signed +int 64+ minimum through
      # unsigned +uint 64+ maximum
      # ({docs/wire-codec.md}[link:../../../docs/wire-codec.md] § Type
      # Mapping #3, the +fixint+ / +int 8..64+ / +uint 8..64+ union).
      # Anchored as a +Range+ so {primitive_type?} stays a single
      # dispatch line. This is the codec's encode domain — not to
      # be confused with the Handle id range, which lives on
      # +Kobako::Handle+ as +MIN_ID+ / +MAX_ID+ (1..2^31 − 1) and
      # represents a different concept entirely.
      MSGPACK_INT_RANGE = (-(2**63)..((2**64) - 1))

      # Codec-type predicate
      # ({docs/wire-codec.md}[link:../../../docs/wire-codec.md] § Type
      # Mapping). Returns +true+ when +value+ belongs to the closed
      # 12-entry codec type set — +nil+, +TrueClass+, +FalseClass+,
      # +Integer+ (in the +i64..u64+ value domain), +Float+, +String+,
      # +Symbol+, +Kobako::Handle+, +Array+ whose every element is itself
      # representable, or +Hash+ whose every key and value are
      # representable. Integers outside the codec's signed-64 /
      # unsigned-64 union are rejected so the predicate agrees with the
      # msgpack gem's encode-time +RangeError+ behaviour the codec
      # already surfaces as {UnsupportedType}.
      def representable?(value)
        primitive_type?(value) || container_representable?(value)
      end

      # Deep-walk Array / Hash containers in +value+ and replace every
      # leaf that fails {representable?} with a +Kobako::Handle+
      # allocated from +handler+. The
      # walk only descends through representable container shapes
      # (Array, Hash) one structural level at a time; a non-representable
      # leaf is wrapped as-is without inspecting its internal structure.
      # An existing +Kobako::Handle+ is representable and passes through
      # unchanged — auto-wrap never re-wraps a Handle.
      #
      # +value+ may be any Ruby value; +handler+ must respond to
      # +#alloc(object) -> Kobako::Handle+ (a host-side
      # +Kobako::Catalog::Handles+). Returns a structurally equivalent value
      # whose leaves are either representable or +Kobako::Handle+
      # tokens.
      #
      # The block bodies spell +Utils.deep_wrap+ explicitly rather than
      # the unqualified +deep_wrap+ because +module_function+ makes the
      # instance copy of these helpers private; an implicit receiver
      # inside a block would resolve against the enclosing +self+
      # (still +Utils+ at definition time, but the qualified form keeps
      # the dispatch readable when the recursive call sits inside a
      # Proc captured from elsewhere).
      def deep_wrap(value, handler)
        case value
        when ::Array then value.map { |element| Utils.deep_wrap(element, handler) }
        when ::Hash  then value.transform_values { |val| Utils.deep_wrap(val, handler) }
        else
          representable?(value) ? value : handler.alloc(value)
        end
      end

      # Deep-walk Array / Hash containers in +value+ and replace every
      # +Kobako::Handle+ leaf with the host-side object +handler+ resolves
      # it to. The symmetric inverse of {deep_wrap}: that walk allocates objects
      # into Handles on the host→guest argument path; this walk resolves
      # Handles back to their objects on every guest→host value path — the
      # +#eval+ / +#run+ result and the yield-block result alike. The walk
      # descends through Array elements and Hash keys and values one
      # structural level at a time; any non-Handle leaf passes through
      # unchanged.
      #
      # +value+ is a decoded Ruby value (a Handle here is a wire-decoded
      # +Kobako::Handle+, never a guest-forged one); +handler+ must
      # respond to +#fetch(id) -> object+ (a host-side
      # +Kobako::Catalog::Handles+). +handler.fetch+ raises
      # +Kobako::SandboxError+ for an id with no live binding, the
      # corrupted-runtime fallback.
      def deep_restore(value, handler)
        case value
        when ::Array then value.map { |element| Utils.deep_restore(element, handler) }
        when ::Hash
          value.to_h { |key, val| [Utils.deep_restore(key, handler), Utils.deep_restore(val, handler)] }
        when Kobako::Handle then handler.fetch(value.id)
        else value
        end
      end

      # The non-container branch of {representable?}: returns +true+ for
      # the scalar leaves and an existing Handle. Not part of the
      # public surface; reach for {representable?} instead.
      def primitive_type?(value)
        case value
        when ::NilClass, ::TrueClass, ::FalseClass, ::Float, ::String, ::Symbol, Kobako::Handle then true
        when ::Integer then MSGPACK_INT_RANGE.cover?(value)
        else false
        end
      end

      # The container branch of {representable?}: recurses into Array
      # elements and Hash key+value pairs through the public
      # {representable?}. Not part of the public surface; reach for
      # {representable?} instead.
      def container_representable?(value)
        case value
        when ::Array then value.all? { |element| Utils.representable?(element) }
        when ::Hash  then value.all? { |key, val| Utils.representable?(key) && Utils.representable?(val) }
        else false
        end
      end
    end
  end
end
