# frozen_string_literal: true

require "msgpack"

require_relative "error"
require_relative "ext_types"

module Kobako
  module Codec
    module Encoder # :nodoc:
      # The rescue maps the two violations the factory's +BasicObject+ guard
      # does not reach: an integer outside i64..u64 (+RangeError+) and any
      # packer-internal +NoMethodError+.
      #
      # The caller bounds +value+'s nesting first (Nesting): the packer takes
      # no depth limit and walks a list or map in frames that carry no stack
      # guard, so a value nesting without end — a reference cycle necessarily
      # does — exhausts the machine stack instead of raising.
      def self.encode(value)
        FACTORY.dump(value)
      rescue ::RangeError, ::NoMethodError => e
        raise UnsupportedTypeError, e.message
      end
    end
  end
end
