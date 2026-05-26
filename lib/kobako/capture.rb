# frozen_string_literal: true

module Kobako
  # Host-side captured prefix of guest stdout / stderr produced during a
  # single +Kobako::Sandbox+ invocation, paired with the truncation flag
  # the WASI pipe sets when the guest wrote past the configured per-channel
  # cap ({docs/behavior.md B-04}[link:../../docs/behavior.md]).
  #
  # Immutable value object: the captured bytes and the truncation flag
  # always travel together and the instance is frozen on construction.
  # Construct via +Capture.new(bytes:, truncated:)+ for the ext-provided
  # binary bytes (the constructor handles the UTF-8 / ASCII-8BIT fallback)
  # or reach +Capture::EMPTY+ for the pre-invocation sentinel that
  # +Sandbox+ uses before any invocation has executed.
  class Capture
    attr_reader :bytes

    # Build a Capture wrapping +bytes+ (the captured prefix as a String) and
    # +truncated+ (whether the originating WASI pipe reported the cap was
    # hit). Coerces +bytes+ to UTF-8 when they are valid UTF-8, otherwise
    # falls back to ASCII-8BIT so invalid sequences remain inspectable
    # without raising; +bytes+ is duplicated, never mutated. Freezes the
    # instance so callers cannot mutate the pair.
    def initialize(bytes:, truncated:)
      copy = bytes.dup.force_encoding(Encoding::UTF_8)
      copy.force_encoding(Encoding::ASCII_8BIT) unless copy.valid_encoding?
      @bytes = copy
      @truncated = truncated
      freeze
    end

    # Returns +true+ iff the underlying capture channel exceeded its
    # configured cap during the originating +Sandbox+ invocation
    # ({docs/behavior.md B-04}[link:../../docs/behavior.md]).
    def truncated? = @truncated

    # Pre-invocation sentinel ({docs/behavior.md B-05}[link:../../docs/behavior.md]).
    # Empty UTF-8 bytes and +truncated? == false+; reused by every fresh
    # +Sandbox+ and by +Sandbox+ between invocations to denote "no capture
    # yet".
    EMPTY = new(bytes: "", truncated: false)
  end
end
