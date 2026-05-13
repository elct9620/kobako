# frozen_string_literal: true

module Kobako
  module Wire
    module Codec
      # Base class for all wire-codec faults raised by the pure-Ruby host codec.
      #
      # The wire codec implements the binary contract pinned in SPEC.md
      # (Wire Codec → Type Mapping). Every wire violation surfaces as a
      # subclass of {Error} so callers can pattern-match on the specific
      # fault while still rescuing all codec faults via this base class.
      #
      # Higher layers (e.g. the Sandbox dispatch loop) translate these into
      # the public {Kobako::SandboxError} / {Kobako::TrapError} taxonomy.
      class Error < StandardError; end

      # Input ended before the type prefix or payload was fully consumed.
      class Truncated < Error; end

      # The type byte at the current position is not in the 12-entry kobako
      # type mapping (e.g. an unknown ext code, or a reserved msgpack tag).
      class InvalidType < Error; end

      # A msgpack `str` payload was not valid UTF-8, or an ext 0x00 Symbol
      # payload was not valid UTF-8 — both are wire violations per SPEC.
      class InvalidEncoding < Error; end

      # The encoder was handed a Ruby object whose type has no wire
      # representation (e.g. Range, Time). Higher layers may catch this
      # and re-route the value through Handle allocation, but at the
      # codec level it is a hard error.
      class UnsupportedType < Error; end
    end
  end
end
