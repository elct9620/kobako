# frozen_string_literal: true

module Kobako
  module Snippet
    # Kobako::Snippet::Binary — value object representing a single
    # +#preload(binary:)+ entry held by +Kobako::Catalog::Snippets+
    # ({docs/behavior.md B-32}[link:../../../docs/behavior.md]).
    #
    # The +body+ is RITE bytecode (as emitted by +mrbc+) carried as an
    # +ASCII_8BIT+ String so msgpack-ruby encodes it as +bin+ family on
    # the wire ({docs/wire-codec.md Invocation channels}[link:../../../docs/wire-codec.md]).
    # The host treats the bytes as opaque — the snippet's canonical
    # name, when present, lives in the bytecode's embedded +debug_info+
    # and is resolved by the guest at load time; structural validation
    # ({docs/behavior.md E-37 / E-38}[link:../../../docs/behavior.md])
    # is deferred to the first invocation's guest replay.
    #
    # The class is a +Data.define+ subclass — frozen and value-equal.
    # Callers (chiefly +Catalog::Snippets+) construct instances via keyword
    # form +Binary.new(body: ...)+. Wire-form construction is the
    # registry's responsibility.
    class Binary < Data.define(:body)
      # The +kind+ field value carried by bytecode snippets in their
      # Frame 3 wire envelope entry
      # ({docs/wire-codec.md Invocation channels}[link:../../../docs/wire-codec.md]).
      KIND = "bytecode"
    end
  end
end
