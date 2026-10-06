# frozen_string_literal: true

module Kobako
  # A guest-held reference to a host object, named by id. Host code cannot
  # construct one, so no Handle is ever made from a caller-chosen id.
  class Handle < Data.define(:id)
    # The smallest id; 0 never names a Handle.
    MIN_ID = 1
    # The largest id, the positive half of a signed 32-bit integer; an
    # invocation needing more raises HandleExhaustedError.
    MAX_ID = 0x7fff_ffff

    def initialize(id:) # :nodoc:
      raise ArgumentError, "Handle id must be Integer" unless id.is_a?(Integer)
      raise ArgumentError, "Handle id #{id} out of range [#{MIN_ID}, #{MAX_ID}]" unless id.between?(MIN_ID, MAX_ID)

      super
    end

    private_class_method :new
    undef_method :with

    def self.restore(id) # :nodoc:
      allocate.tap { |handle| handle.send(:initialize, id: id) }
    end
  end
end
