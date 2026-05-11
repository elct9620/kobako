# frozen_string_literal: true

# Shared byte-builders for tests that drive Sandbox#decode_outcome and the
# Wire::Envelope codec directly. Methods reference Kobako::Wire::Envelope
# at call time only, so this file can be required from tests that opt out
# of the full +require "kobako"+ chain (which would pull in the native ext).
module OutcomeBytesHelpers
  def build_outcome_bytes(tag, body)
    bytes = String.new(encoding: Encoding::ASCII_8BIT)
    bytes << tag.chr(Encoding::ASCII_8BIT)
    bytes << body
    bytes
  end

  def panic_outcome_bytes(origin:, klass:, message:, backtrace: [])
    panic = Kobako::Wire::Envelope::Panic.new(
      origin: origin, klass: klass, message: message, backtrace: backtrace
    )
    build_outcome_bytes(
      Kobako::Wire::Envelope::OUTCOME_TAG_PANIC,
      Kobako::Wire::Envelope.encode_panic(panic)
    )
  end
end
