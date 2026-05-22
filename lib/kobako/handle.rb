# frozen_string_literal: true

module Kobako
  # Wire-level value object for an ext-0x01 Capability Handle, used in both
  # directions across the Sandbox boundary: as a Service method's return
  # value (guest→host return path; {docs/behavior.md B-14}[link:../../docs/behavior.md])
  # and as a +#run+ argument auto-wrapped by the host
  # ({docs/behavior.md B-34}[link:../../docs/behavior.md]).
  #
  # SPEC pins the binary layout to fixext 4 with a 4-byte big-endian u32
  # payload ({docs/wire-codec.md}[link:../../docs/wire-codec.md]
  # § Ext Types → ext 0x01). ID 0 is reserved as the invalid sentinel;
  # the maximum valid ID is 0x7fff_ffff (2^31 - 1).
  #
  # This is intentionally a thin value object built on +Data.define+ so
  # equality, hash, and immutability are inherited. The mruby counterpart
  # +Kobako::Handle+ lives inside the Wasm guest under the same canonical
  # name and shares neither code nor instances with this host-side class.
  Handle = Data.define(:id) do
    # +MIN_ID+ / +MAX_ID+ live on the Handle class (defined below this
    # block), not in this block's binding — Data.define's block scope
    # resolves bare constants against the enclosing +Kobako+ module, so
    # +MIN_ID+ would raise +NameError+. Use +self.class::CONST+ to
    # reach the constants attached to the Handle class itself. Do not
    # "simplify" this back to bare +MIN_ID+/+MAX_ID+.
    # steep:ignore:start
    def initialize(id:)
      min = self.class::MIN_ID
      max = self.class::MAX_ID
      raise ArgumentError, "Handle id must be Integer" unless id.is_a?(Integer)
      raise ArgumentError, "Handle id #{id} out of range [#{min}, #{max}]" unless id.between?(min, max)

      super
    end
    # steep:ignore:end
  end

  Handle::MIN_ID = 1
  Handle::MAX_ID = 0x7fff_ffff
end
