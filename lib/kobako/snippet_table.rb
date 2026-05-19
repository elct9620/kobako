# frozen_string_literal: true

require "msgpack"

module Kobako
  # Kobako::SnippetTable — per-Sandbox ordered registry of preloaded source
  # snippets ({docs/behavior.md B-32 / B-33}[link:../../docs/behavior.md]).
  #
  # Snippets are stored in insertion order; every Sandbox invocation
  # (+#eval+ or +#run+) replays them against the fresh +mrb_state+ before
  # per-invocation source / entrypoint resolution. Each snippet's +name+
  # is its canonical identity — it is the filename baked into the loaded
  # IREP's +debug_info+ and surfaces in every backtrace frame originating
  # from the snippet as +(snippet:Name):line+; duplicate names would
  # produce ambiguous attribution and are rejected at registration time
  # ({docs/behavior.md E-33}[link:../../docs/behavior.md]).
  #
  # Sealing (B-33) is governed by the owning Sandbox — the table itself
  # is append-only and exposes no mutation API beyond +#register+; the
  # Sandbox guards +#register+ behind the seal check before delegating.
  class SnippetTable
    # Ruby constant-name pattern enforced on snippet names
    # ({docs/behavior.md E-34}[link:../../docs/behavior.md]).
    NAME_PATTERN = /\A[A-Z]\w*\z/

    # The only legal value of the +kind+ field on a Frame 3 snippet entry
    # in this revision; the slot exists as a forward-compatibility point
    # for the future bytecode preload path
    # ({docs/wire-codec.md Invocation channels}[link:../../docs/wire-codec.md]).
    SOURCE_KIND = "source"

    def initialize
      @entries = {} # : Hash[Symbol, String]
    end

    # Encode the registered snippets as Frame 3 msgpack bytes
    # ({docs/wire-codec.md Invocation channels}[link:../../docs/wire-codec.md]).
    # Layout: msgpack array, one msgpack map per snippet with string keys
    # +"name"+, +"kind"+, +"body"+. Mandatory-presence — an empty table
    # encodes as an empty array, never absent. Returns a binary +String+
    # of msgpack bytes.
    def encoded_frame3
      entries = @entries.map do |name, body|
        { "name" => name.to_s, "kind" => SOURCE_KIND, "body" => body }
      end
      MessagePack.pack(entries)
    end

    # Register +code+ under the canonical Symbol form of +name+. +code+ is
    # the mruby source as a String; the bytes are re-encoded as UTF-8 and
    # detached from the caller's reference. +name+ is a Symbol or String
    # matching +NAME_PATTERN+. Returns the Symbol form of +name+.
    #
    # Raises +ArgumentError+ when +name+ is malformed (E-34) or duplicates
    # an already-registered snippet (E-33).
    def register(code, name)
      name_sym = normalize_name(name)
      raise ArgumentError, "snippet #{name_sym.inspect} already preloaded" if @entries.key?(name_sym)

      @entries[name_sym] = code.dup.force_encoding(Encoding::UTF_8)
      name_sym
    end

    # Iterate over registered snippets in insertion order. Yields
    # +[name_sym, code_string]+ pairs. Returns an Enumerator when no block
    # is given.
    def each(&)
      @entries.each(&)
    end

    # All registered snippet names, in insertion order.
    def names
      @entries.keys
    end

    # Number of registered snippets.
    def size
      @entries.size
    end

    # Whether no snippets are registered.
    def empty?
      @entries.empty?
    end

    # Whether a snippet with +name+ (Symbol or String accepted) is
    # already registered. Used by tests and the Sandbox to detect
    # duplicates before delegating to +#register+ where a more specific
    # error message can be produced.
    def key?(name)
      @entries.key?(name.to_sym)
    end

    private

    def normalize_name(name)
      unless name.is_a?(Symbol) || name.is_a?(String)
        raise ArgumentError, "snippet name must be a Symbol or String, got #{name.class}"
      end

      name_str = name.to_s
      unless NAME_PATTERN.match?(name_str)
        raise ArgumentError,
              "snippet name must match #{NAME_PATTERN.inspect} (got #{name.inspect})"
      end

      name_str.to_sym
    end
  end
end
