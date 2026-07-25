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
    # carrier this Source stays pure and +Catalog::Snippets#entries+ reads
    # its attributes off the outside rather than asking it to project itself.
    class Source < Data.define(:name, :body)
      # Names the snippet form the guest replays this entry as. The wire's
      # discriminant byte is assigned by the core envelope, not here.
      KIND = :source
    end
  end
end
