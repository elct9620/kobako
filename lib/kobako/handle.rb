# frozen_string_literal: true

module Kobako
  # Wire-level value object for an ext-0x01 Capability Handle, used in both
  # directions across the Sandbox boundary: as a Service method's return
  # value (guest→host return path) and as a +#run+ argument auto-wrapped
  # by the host.
  #
  # SPEC pins the binary layout to fixext 4 with a 4-byte big-endian u32
  # payload ({docs/wire-codec.md}[link:../../docs/wire-codec.md]
  # § Ext Types → ext 0x01). ID 0 is reserved as the invalid sentinel;
  # the maximum valid ID is 0x7fff_ffff (2^31 - 1).
  #
  # The constructor is internal to the Host Gem. +Kobako::Handle.new+ is
  # privatised so Host App code cannot fabricate a Handle from a bare
  # integer; legitimate Handle instances enter Host App code only as
  # fields on raised error objects. +#with+ — Data's copy-with-changes
  # constructor — is removed for the same reason: a legitimate Handle
  # must not derive a sibling with a caller-chosen id. The Host Gem itself constructs
  # Handles through +.restore+, which exists at exactly two call
  # sites: +Kobako::Codec::Factory#unpack_handle+ (wire decode) and
  # +Kobako::Codec::HandleWalk.deep_wrap+ / +Kobako::Transport::Dispatcher#wrap_as_handle+
  # (allocator paths). Both live inside +lib/kobako/+ and are not part
  # of any public surface.
  #
  # The mruby counterpart +Kobako::Handle+ lives inside the Wasm guest
  # under the same canonical name and shares neither code nor instances
  # with this host-side class.
  class Handle < Data.define(:id)
    # Inclusive lower bound on the wire Handle ID. ID 0 is reserved as
    # the invalid sentinel and is never allocated.
    MIN_ID = 1
    # Inclusive upper bound on the wire Handle ID. The cap matches the
    # u32 signed-positive range so Handle IDs fit in a signed integer
    # on either side of the wire without re-encoding.
    MAX_ID = 0x7fff_ffff

    def initialize(id:)
      raise ArgumentError, "Handle id must be Integer" unless id.is_a?(Integer)
      raise ArgumentError, "Handle id #{id} out of range [#{MIN_ID}, #{MAX_ID}]" unless id.between?(MIN_ID, MAX_ID)

      super
    end

    private_class_method :new
    undef_method :with

    # Host Gem–internal factory. Allocates the Data instance through
    # +Class#allocate+ and dispatches +#initialize+ explicitly so the
    # invariant checks still run, while keeping the public +.new+
    # privatised against Host App callers.
    #
    # Two collaborators call this: the codec when an ext 0x01 frame is
    # decoded off the wire, and the allocator paths when a host-side
    # Ruby object is registered into the Sandbox's +Catalog::Handles+. Both
    # paths live inside +lib/kobako/+ and treat this method as a
    # package-private constructor.
    def self.restore(id)
      allocate.tap { |handle| handle.send(:initialize, id: id) }
    end
  end
end
