# frozen_string_literal: true

require_relative "error"
require_relative "handle"
require_relative "exception"
require_relative "encoder"
require_relative "decoder"

module Kobako
  module Wire
    # Envelope-layer encoders/decoders for the kobako wire contract.
    #
    # SPEC.md → Wire Contract pins the logical shape of every host↔guest
    # message and SPEC.md → Wire Codec → Envelope Frame Layout pins the
    # binary framing. This module assembles the four envelope kinds
    # (Request, Response, Result, Panic) and the outer Outcome wrapper on
    # top of the lower-level {Encoder} / {Decoder} primitives.
    #
    # The envelope objects are plain value objects; they hold the logical
    # fields and validate basic shape invariants. The actual byte layout
    # (msgpack array vs map, field ordering, outcome-tag bytes) is owned
    # by the +Envelope+ module's class methods so the Encoder/Decoder
    # primitives stay byte-only and SPEC's framing rules live in one place.
    module Envelope
      # ---------------- Outcome tag bytes (SPEC.md Outcome Envelope) -----

      # First byte of the OUTCOME_BUFFER for a Result envelope.
      OUTCOME_TAG_RESULT = 0x01
      # First byte of the OUTCOME_BUFFER for a Panic envelope.
      OUTCOME_TAG_PANIC  = 0x02

      # ---------------- Response status bytes (SPEC.md Response Shape) ---

      # Response variant marker for the success branch.
      STATUS_OK    = 0
      # Response variant marker for the error branch.
      STATUS_ERROR = 1

      # =================================================================
      # Value objects and their encode/decode helpers (one file per class)
      # =================================================================

      require_relative "envelope/request"
      require_relative "envelope/response"
      require_relative "envelope/result"
      require_relative "envelope/panic"
      require_relative "envelope/outcome"
    end
  end
end
