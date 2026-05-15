# frozen_string_literal: true

# Shared byte-builders for tests that drive Kobako::Outcome and the
# Wire::Codec directly. Methods reference Kobako::Outcome and
# Kobako::Wire::Codec at call time only, so this file can be required
# from tests that opt out of the full +require "kobako"+ chain (which
# would pull in the native ext).
#
# The host never emits Outcome bytes in production — the Rust guest
# does. These helpers exist only to assemble test fixtures: a known-good
# panic body for the cross-language oracle, a hand-rolled outcome for
# error-attribution decode tests, etc.
module OutcomeBytesHelpers
  def build_outcome_bytes(tag, body)
    bytes = String.new(encoding: Encoding::ASCII_8BIT)
    bytes << tag.chr(Encoding::ASCII_8BIT)
    bytes << body
    bytes
  end

  # Encode a Panic value object into the wire-shape msgpack map bytes
  # the guest would emit on the failure branch (SPEC.md Outcome
  # Envelope → Panic). +"backtrace"+ is omitted when empty; +"details"+
  # is omitted when nil — matching the Rust encoder so byte-for-byte
  # oracle round-trips stay identical.
  def encode_panic_body(panic)
    map = { "origin" => panic.origin, "class" => panic.klass, "message" => panic.message }
    map["backtrace"] = panic.backtrace unless panic.backtrace.empty?
    map["details"]   = panic.details unless panic.details.nil?
    Kobako::Wire::Codec::Encoder.encode(map)
  end

  def panic_outcome_bytes(origin:, klass:, message:, backtrace: [])
    panic = Kobako::Outcome::Panic.new(
      origin: origin, klass: klass, message: message, backtrace: backtrace
    )
    build_outcome_bytes(Kobako::Outcome::TYPE_PANIC, encode_panic_body(panic))
  end
end
