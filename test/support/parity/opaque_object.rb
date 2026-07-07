# frozen_string_literal: true

module Parity
  # A deliberately non-wire-representable host object with a
  # scenario-declared identity — the Ruby interpretation of the closed
  # +opaque+ tag. It wraps into a capability Handle on every crossing,
  # and +label+ is its only Service surface, mirroring the Rust
  # runner's OpaqueStub.
  OpaqueObject = Struct.new(:label)
end
