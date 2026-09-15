# frozen_string_literal: true

require_relative "error"

module Kobako
  module Codec
    # The structural nesting bound
    # ({docs/wire/payload-msgpack.md}[link:../../../docs/wire/payload-msgpack.md]
    # § Structural Nesting Depth), checked before a value reaches the
    # packer. The packer takes no depth limit and walks a list or map in
    # frames that carry no stack guard, so a value it cannot finish — a
    # reference cycle necessarily is one — must be refused before it is
    # handed over.
    module Nesting
      # Raise InvalidTypeError when +value+ nests past MAX_NESTING_DEPTH.
      # Depth counts as HandleWalk#representable? counts it, so a value
      # nested exactly to the bound passes.
      def self.assert_within_bound!(value, depth = 0)
        if depth > MAX_NESTING_DEPTH
          raise InvalidTypeError,
                "value nests deeper than #{MAX_NESTING_DEPTH} levels (a reference cycle necessarily does)"
        end

        case value
        when ::Array then value.each { |element| assert_within_bound!(element, depth + 1) }
        when ::Hash  then value.each { |pair| pair.each { |member| assert_within_bound!(member, depth + 1) } }
        end
      end
    end
  end
end
