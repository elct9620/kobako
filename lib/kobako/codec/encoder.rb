# frozen_string_literal: true

require "msgpack"

require_relative "error"
require_relative "ext_types"

module Kobako
  module Codec
    # Module-level entry point for the host side of the kobako wire
    # ({docs/wire/payload-msgpack.md}[link:../../../docs/wire/payload-msgpack.md] § Type Mapping).
    #
    # The codec backbone is the official +msgpack+ gem: integers, floats,
    # strings, arrays, and maps go through the gem's narrowest-encoding
    # logic; the two kobako-specific ext types (0x00 Symbol, 0x01
    # Capability Handle) are registered by ExtTypes on the process-wide
    # factory.
    #
    # Public API is a single function — +.encode+. The codec is stateless;
    # there is no buffer accumulator and no streaming write API. Callers
    # that need to concatenate multiple encodings build the bytes
    # themselves.
    module Encoder
      # Encode +value+ to wire bytes (binary-encoded String).
      # SPEC's 11-entry type mapping is a closed set: a value outside it is
      # rejected as +UnsupportedTypeError+ by the factory's +BasicObject+ guard
      # (ExtTypes#register_unrepresentable), which raises before the msgpack
      # gem can route the value through +to_msgpack+ — so a permissive
      # +method_missing+ object cannot answer that probe and mis-encode. The
      # rescue below maps the two violations the guard does not reach onto the
      # same error: an integer outside i64..u64 (+RangeError+) and any
      # packer-internal +NoMethodError+.
      #
      # A value that nests without bound — a reference cycle necessarily
      # does — exhausts the packer's own recursion instead, which Ruby
      # reports outside +StandardError+. Mapping it keeps an unwritable
      # value a wire violation the dispatch boundary can answer, rather than
      # one that escapes every caller's rescue and traps the invocation.
      #
      # The refusal is spent once per thread: a thread that has absorbed one
      # such overflow aborts on the next instead of raising, and a Hash cycle
      # never reaches Ruby at all — the packer walks a Hash through C frames
      # that carry no stack guard. Bounding the walk before the packer is
      # handed the value is what would make the refusal repeatable.
      def self.encode(value)
        FACTORY.dump(value)
      rescue ::RangeError, ::NoMethodError => e
        raise UnsupportedTypeError, e.message
      rescue ::SystemStackError
        raise InvalidTypeError, "value nests deeper than this host can write (a reference cycle necessarily does)"
      end
    end
  end
end
