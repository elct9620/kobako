# frozen_string_literal: true

require_relative "../handle"
require_relative "../errors"

module Kobako
  module Codec
    module HandleWalk # :nodoc:
      module_function

      # Inclusive Integer range the msgpack gem encodes without raising
      # +RangeError+ at encode time — signed +int 64+ minimum through
      # unsigned +uint 64+ maximum
      # ({docs/wire/payload-msgpack.md}[link:../../../docs/wire/payload-msgpack.md] § Type
      # Mapping #3, the +fixint+ / +int 8..64+ / +uint 8..64+ union).
      # Anchored as a +Range+ so #primitive_type? stays a single
      # dispatch line. This is the codec's encode domain — not to
      # be confused with the Handle id range, which lives on
      # +Kobako::Handle+ as +MIN_ID+ / +MAX_ID+ (1..2^31 − 1) and
      # represents a different concept entirely.
      MSGPACK_INT_RANGE = (-(2**63)..((2**64) - 1))

      # Whether +value+ falls in the codec's type set
      # ({docs/wire/payload-msgpack.md}[link:../../../docs/wire/payload-msgpack.md] § Type
      # Mapping). The +depth+ bound makes a self-referential container —
      # reachable as a cyclic Hash key on the +#run+ argument path — read as
      # non-representable rather than looping.
      def representable?(value, depth = 0)
        return false if depth > MAX_NESTING_DEPTH

        primitive_type?(value) || container_representable?(value, depth)
      end

      # Only Hash values are wrapped: a non-representable key raises rather
      # than crossing as a token the guest→host restore walk could not
      # round-trip.
      #
      # Recursive calls spell the +HandleWalk.+ receiver so the dispatch
      # stays valid even when a block is captured and run under a
      # different +self+ (+module_function+ privatizes the instance copies).
      def deep_wrap(value, handles, depth = 0)
        guard_nesting!(depth)
        case value
        when ::Array then value.map { |element| HandleWalk.deep_wrap(element, handles, depth + 1) }
        when ::Hash  then HandleWalk.deep_wrap_hash(value, handles, depth)
        else
          representable?(value) ? value : handles.alloc(value)
        end
      end

      # A reference cycle necessarily trips the nesting bound, so a +#run+
      # argument fails as a clean +Kobako::SandboxError+ rather than an
      # unbounded host recursion.
      def guard_nesting!(depth)
        return unless depth > MAX_NESTING_DEPTH

        raise Kobako::SandboxError,
              "a #run argument nests deeper than #{MAX_NESTING_DEPTH} levels within the Run payload and " \
              "cannot cross the sandbox boundary (possible reference cycle)"
      end

      # A stateful object may cross the boundary as a Hash value but not as
      # a key — the one deliberate asymmetry with the guest→host restore
      # walk, which resolves Handle keys the guest built.
      def deep_wrap_hash(hash, handles, depth)
        wrapped = {} # : Hash[untyped, untyped]
        hash.each do |key, val|
          unless HandleWalk.representable?(key, depth + 1)
            raise Kobako::SandboxError,
                  "a Hash passed to #run has a key that cannot cross the sandbox boundary " \
                  "(#{key.class}); only wire-representable values may be Hash keys"
          end
          wrapped[key] = HandleWalk.deep_wrap(val, handles, depth + 1)
        end
        wrapped
      end

      # The inverse of #deep_wrap. A Handle here was decoded off the wire,
      # never forged by the guest, and +handles.fetch+ refuses an id with no
      # live binding.
      def deep_restore(value, handles)
        case value
        when ::Array then value.map { |element| HandleWalk.deep_restore(element, handles) }
        when ::Hash
          # Rebuilt with each key restored: two distinct Handle keys that
          # resolve to equal host objects collapse to the later pair, as in
          # any Ruby Hash. The guest authored this payload, so that collapse
          # is its own concern, not a fidelity guarantee the host owes it.
          value.to_h { |key, val| [HandleWalk.deep_restore(key, handles), HandleWalk.deep_restore(val, handles)] }
        when Kobako::Handle then handles.fetch(value.id)
        else value
        end
      end

      def primitive_type?(value)
        case value
        when ::NilClass, ::TrueClass, ::FalseClass, ::Float, ::String, ::Symbol, Kobako::Handle then true
        when ::Integer then MSGPACK_INT_RANGE.cover?(value)
        else false
        end
      end

      def container_representable?(value, depth)
        case value
        when ::Array then value.all? { |element| HandleWalk.representable?(element, depth + 1) }
        when ::Hash  then value.all? { |pair| pair.all? { |member| HandleWalk.representable?(member, depth + 1) } }
        else false
        end
      end
    end
  end
end
