# frozen_string_literal: true

# Host-side implementation of the kobako wire codec.
#
# Backed by the official +msgpack+ gem via {Kobako::Wire::Factory}; the
# {Encoder} / {Decoder} classes are thin wrappers that register the two
# kobako-specific ext types (0x01 Capability Handle and 0x02 Exception
# envelope) on a +MessagePack::Factory+. The module is intentionally
# self-contained — it does not depend on the native extension or on
# +lib/kobako.rb+ — so it can be required directly from tests that run
# on a clean checkout (no compiled artifacts).
#
# See SPEC.md → Wire Codec for the binary contract this codec implements.
module Kobako
  # Host-side MessagePack codec for the kobako wire contract.
  # See SPEC.md → Wire Codec for the binary layout this namespace implements.
  module Wire
  end
end

require_relative "wire/error"
require_relative "wire/handle"
require_relative "wire/exception"
require_relative "wire/factory"
require_relative "wire/encoder"
require_relative "wire/decoder"
require_relative "wire/envelope"
