# frozen_string_literal: true

module Kobako
  module Snippet
    # Kobako::Snippet::Source — value object representing a single
    # +#preload(code:, name:)+ entry held by +Kobako::Snippet::Table+
    # ({docs/behavior.md B-32}[link:../../../docs/behavior.md]).
    #
    # +name+ is the canonical +Symbol+ identity baked into the loaded
    # IREP's +debug_info+; backtrace frames originating in this snippet
    # surface as +(snippet:Name):line+. +body+ is the UTF-8 mruby source
    # detached from the caller's reference at +Table#register+ time so
    # later mutation of the original String cannot bleed through.
    #
    # The class is a +Data.define+ subclass — frozen, value-equal, and
    # carries no mutation API. Callers (chiefly +Table+) construct
    # instances via keyword form +Source.new(name: ..., body: ...)+.
    class Source < Data.define(:name, :body)
      # The +kind+ field value carried by source snippets in their Frame
      # 3 wire envelope entry
      # ({docs/wire-codec.md Invocation channels}[link:../../../docs/wire-codec.md]).
      KIND = "source"

      # Produce the msgpack map this snippet contributes to the Frame 3
      # array. The +body+ is a UTF-8 String and ships as msgpack +str+;
      # +name+ is downcast to its String form so the wire decoder sees a
      # uniform string-keyed map.
      def to_wire
        { "name" => name.to_s, "kind" => KIND, "body" => body }
      end
    end
  end
end
