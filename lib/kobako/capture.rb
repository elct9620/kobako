# frozen_string_literal: true

module Kobako
  # Host-side captured prefix of guest stdout / stderr produced during a
  # single +Kobako::Sandbox#run+, paired with the truncation flag the WASI
  # pipe sets when the guest wrote past the configured per-channel cap
  # ({SPEC.md B-04}[link:../../SPEC.md]).
  #
  # Immutable value object: the captured bytes and the truncation flag
  # always travel together and the instance is frozen on construction.
  # Construct via +Capture.from_ext+ for ext-provided binary bytes (handles
  # UTF-8 / ASCII-8BIT fallback) or reach +Capture::EMPTY+ for the pre-run
  # sentinel that +Sandbox+ uses before any +#run+ has executed.
  class Capture
    attr_reader :bytes

    # Build a Capture wrapping +bytes+ (the captured prefix as a String) and
    # +truncated+ (whether the originating WASI pipe reported the cap was
    # hit). Freezes the instance so callers cannot mutate the pair.
    def initialize(bytes:, truncated:)
      @bytes = bytes
      @truncated = truncated
      freeze
    end

    # Returns +true+ iff the underlying capture channel exceeded its
    # configured cap during the originating +Sandbox#run+
    # ({SPEC.md B-04}[link:../../SPEC.md]).
    def truncated? = @truncated

    # Construct a Capture from ext-provided binary bytes. Coerces +bytes+
    # to UTF-8 when the bytes are valid UTF-8, otherwise falls back to
    # ASCII-8BIT so invalid sequences remain inspectable without raising.
    # +bytes+ is not mutated.
    def self.from_ext(bytes, truncated)
      copy = bytes.dup.force_encoding(Encoding::UTF_8)
      copy.force_encoding(Encoding::ASCII_8BIT) unless copy.valid_encoding?
      new(bytes: copy, truncated: truncated)
    end

    # Pre-run sentinel ({SPEC.md B-05}[link:../../SPEC.md]). Empty UTF-8
    # bytes and +truncated? == false+; reused by every fresh +Sandbox+ and
    # by +Sandbox#run+ between invocations to denote "no capture yet".
    EMPTY = new(bytes: "", truncated: false)
  end
end
