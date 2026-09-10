# frozen_string_literal: true

module Kobako
  # A Capability Handle as it crosses the wire (ext 0x01, laid out in
  # {docs/wire-codec.md}[link:../../docs/wire-codec.md]), in either
  # direction. Host App code cannot construct one: +.new+ is private and
  # +#with+ removed, so no Handle is ever made from a caller-chosen id.
  # The guest's +Kobako::Handle+ shares only the name.
  class Handle < Data.define(:id)
    # ID 0 is the invalid sentinel and is never allocated.
    MIN_ID = 1
    # The positive half of a signed 32-bit integer, so an id fits either
    # side of the wire unchanged.
    MAX_ID = 0x7fff_ffff

    def initialize(id:)
      raise ArgumentError, "Handle id must be Integer" unless id.is_a?(Integer)
      raise ArgumentError, "Handle id #{id} out of range [#{MIN_ID}, #{MAX_ID}]" unless id.between?(MIN_ID, MAX_ID)

      super
    end

    private_class_method :new
    undef_method :with

    # The Host Gem's own constructor, for the wire decoder and the Handle
    # allocator; it runs the same id checks +.new+ would.
    def self.restore(id)
      allocate.tap { |handle| handle.send(:initialize, id: id) }
    end
  end
end
