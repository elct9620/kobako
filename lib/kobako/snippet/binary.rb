# frozen_string_literal: true

module Kobako
  module Snippet
    # Kobako::Snippet::Binary — value object representing a single
    # +#preload(binary:)+ entry held by +Kobako::Catalog::Snippets+.
    #
    # The +body+ is RITE bytecode (as emitted by +mrbc+) carried as an
    # +ASCII_8BIT+ String so msgpack-ruby encodes it as +bin+ family on
    # the wire ({docs/wire-codec.md Invocation channels}[link:../../../docs/wire-codec.md]).
    # The host treats the bytes as opaque — the snippet's canonical
    # name, when present, lives in the bytecode's embedded +debug_info+
    # and is resolved by the guest at load time; structural validation
    # is deferred to the first invocation's guest replay.
    #
    # The class is a +Data.define+ subclass — frozen and value-equal.
    # Callers (chiefly +Catalog::Snippets+) construct instances via keyword
    # form +Binary.new(body: ...)+. Wire-form construction is the
    # registry's responsibility.
    class Binary < Data.define(:body)
      # Names the snippet form the guest replays this entry as. The wire's
      # discriminant byte is assigned by the core envelope, not here.
      KIND = :bytecode
    end
  end
end
