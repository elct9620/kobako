# frozen_string_literal: true

module Kobako
  module Snippet
    # Kobako::Snippet::Source — value object representing a single
    # +#preload(code:, name:)+ entry held by +Kobako::Catalog::Snippets+.
    #
    # +name+ is the canonical +Symbol+ identity baked into the loaded
    # IREP's +debug_info+; backtrace frames originating in this snippet
    # surface as +(snippet:Name):line+. +body+ is the UTF-8 mruby source
    # detached from the caller's reference at +Catalog::Snippets#register+
    # time so later mutation of the original String cannot bleed through.
    #
    # The class is a +Data.define+ subclass — frozen, value-equal, and
    # carries no mutation API. Callers (chiefly +Catalog::Snippets+)
    # construct instances via keyword form +Source.new(name: ..., body: ...)+.
    # Wire-form construction is the registry's responsibility: as a leaf
    # carrier this Source stays pure and +Catalog::Snippets#encode+ reads
    # its attributes off the outside rather than asking it to self-encode.
    class Source < Data.define(:name, :body)
      # The +kind+ field value carried by source snippets in their Frame
      # 3 wire envelope entry
      # ({docs/wire-codec.md Invocation channels}[link:../../../docs/wire-codec.md]).
      KIND = "source"
    end
  end
end
