# frozen_string_literal: true

require_relative "../codec"
require_relative "exposure"

module Kobako
  module Transport
    # The walk a #run argument takes on its way to the guest: a leaf with no
    # wire form becomes a Capability Handle in the invocation's table, and
    # the rest crosses as it stands. An argument the walk refuses is the
    # Host App's mistake, raised as +Marshal.dump+ raises it: +TypeError+
    # for what has no form to cross as, +ArgumentError+ past the depth.
    module ArgumentWrap # :nodoc:
      # Only Hash values are wrapped: a non-representable key raises rather
      # than crossing as a token the guest→host restore walk could not
      # round-trip.
      def self.wrap(value, handles, depth = 0)
        refuse_past_nesting(depth)
        case value
        when ::Array then value.map { |element| wrap(element, handles, depth + 1) }
        when ::Hash  then wrap_hash(value, handles, depth)
        else
          Codec::HandleWalk.representable?(value) ? value : wrap_leaf(value, handles)
        end
      end

      class << self
        private

        # A reference cycle necessarily trips the nesting bound, so an
        # argument fails as a clean +ArgumentError+ rather than an unbounded
        # host recursion.
        def refuse_past_nesting(depth)
          return unless depth > Codec::MAX_NESTING_DEPTH

          raise ArgumentError,
                "a #run argument nests deeper than #{Codec::MAX_NESTING_DEPTH} levels within the Run payload " \
                "and cannot cross the sandbox boundary (possible reference cycle)"
        end

        def wrap_leaf(value, handles)
          refused = Exposure.refused_class_of(value)
          raise TypeError, "a #{refused} cannot cross as a Capability Handle" if refused

          handles.alloc(value)
        end

        # A stateful object may cross the boundary as a Hash value but not as
        # a key — the one deliberate asymmetry with the guest→host restore
        # walk, which resolves Handle keys the guest built.
        def wrap_hash(hash, handles, depth)
          hash.to_h do |key, val|
            unless Codec::HandleWalk.representable?(key, depth + 1)
              raise TypeError,
                    "a Hash passed to #run has a key that cannot cross the sandbox boundary " \
                    "(#{key.class}); only wire-representable values may be Hash keys"
            end
            [key, wrap(val, handles, depth + 1)]
          end
        end
      end
    end
  end
end
