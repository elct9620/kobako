# frozen_string_literal: true

require_relative "codec/error"
require_relative "codec/utils"
require_relative "codec/handle_walk"
require_relative "codec/nesting"
require_relative "codec/state"
require_relative "codec/ext_types"
require_relative "codec/encoder"
require_relative "codec/decoder"

module Kobako
  module Codec # :nodoc:
    # The maximum structural nesting depth the wire represents (the
    # MessagePack ecosystem's bound), shared with the guest +kobako_codec+
    # so both sides cap identically. The host refuses a value nesting past
    # it — a reference cycle necessarily does — before handing it to the
    # packer.
    MAX_NESTING_DEPTH = 128

    # Answers the block's result with whether the decode carried a
    # Capability Handle, so a caller can skip an all-identity restore walk.
    def self.track_handles(&block)
      State.current.track_handles(&block)
    end
  end
end
