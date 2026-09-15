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
      # Raise InvalidTypeError when +value+ nests past MAX_NESTING_DEPTH; a
      # value nested exactly to the bound passes.
      def self.assert_within_bound!(value, depth = 0)
        case value
        when ::Array then assert_members_within_bound!(value, depth)
        when ::Hash
          assert_members_within_bound!(value.keys, depth)
          assert_members_within_bound!(value.values, depth)
        end
      end

      # Measure the members of one container at +depth+, each sitting one
      # level deeper. A member is matched by class rather than asked about
      # itself — it may be a BasicObject — and only one that could be a
      # container is walked into.
      def self.assert_members_within_bound!(members, depth)
        return if members.empty?
        if depth >= MAX_NESTING_DEPTH
          raise InvalidTypeError,
                "value nests deeper than #{MAX_NESTING_DEPTH} levels (a reference cycle necessarily does)"
        end
        return unless members.any?(::Enumerable)

        members.grep(::Enumerable) { |member| assert_within_bound!(member, depth + 1) }
      end
      private_class_method :assert_members_within_bound!
    end
  end
end
