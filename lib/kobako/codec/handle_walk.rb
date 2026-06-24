# frozen_string_literal: true

require_relative "../handle"

module Kobako
  module Codec
    # Substitutes Capability Handles into and out of a Ruby value tree at
    # the host↔guest boundary. {deep_wrap} allocates a +Kobako::Handle+ for
    # each non-wire-representable leaf on the host→guest +#run+ argument
    # path; {deep_restore} resolves each wire-decoded Handle back to its
    # host object on every guest→host value path — the +#eval+ / +#run+
    # result and the yield-block result alike. {representable?} is the
    # by-value codec-type predicate that decides which leaves {deep_wrap}
    # must wrap: the closed 12-entry wire type set
    # ({docs/wire-codec.md}[link:../../../docs/wire-codec.md] § Type
    # Mapping).
    #
    # All helpers are pure except {deep_wrap}, whose only side effect is
    # allocating new Handle ids into the supplied table.
    module HandleWalk
      module_function

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
      # The block bodies spell +HandleWalk.deep_wrap+ explicitly rather
      # than the unqualified +deep_wrap+ because +module_function+ makes
      # the instance copy of these helpers private; an implicit receiver
      # inside a block would resolve against the enclosing +self+
      # (still +HandleWalk+ at definition time, but the qualified form
      # keeps the dispatch readable when the recursive call sits inside a
      # Proc captured from elsewhere).
      def deep_wrap(value, handler)
        case value
        when ::Array then value.map { |element| HandleWalk.deep_wrap(element, handler) }
        when ::Hash  then value.transform_values { |val| HandleWalk.deep_wrap(val, handler) }
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
        when ::Array then value.map { |element| HandleWalk.deep_restore(element, handler) }
        when ::Hash
          value.to_h { |key, val| [HandleWalk.deep_restore(key, handler), HandleWalk.deep_restore(val, handler)] }
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
        when ::Array then value.all? { |element| HandleWalk.representable?(element) }
        when ::Hash  then value.all? { |key, val| HandleWalk.representable?(key) && HandleWalk.representable?(val) }
        else false
        end
      end
    end
  end
end
