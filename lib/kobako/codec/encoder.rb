# frozen_string_literal: true

require "msgpack"

require_relative "error"
require_relative "ext_types"

module Kobako
  module Codec
    module Encoder # :nodoc:
      # Encode +value+ to wire bytes (binary-encoded String).
      # The 11-entry type mapping is a closed set: a value outside it is
      # rejected as +UnsupportedTypeError+ by the factory's +BasicObject+ guard
      # (ExtTypes#register_unrepresentable), which raises before the msgpack
      # gem can route the value through +to_msgpack+ — so a permissive
      # +method_missing+ object cannot answer that probe and mis-encode. The
      # rescue below maps the two violations the guard does not reach onto the
      # same error: an integer outside i64..u64 (+RangeError+) and any
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
