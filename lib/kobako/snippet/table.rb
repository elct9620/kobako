# frozen_string_literal: true

require "msgpack"

require_relative "source"

module Kobako
  module Snippet
    # Kobako::Snippet::Table — per-Sandbox insertion-ordered registry of
    # preloaded snippets
    # ({docs/behavior.md B-32 / B-33}[link:../../../docs/behavior.md]).
    #
    # Entries replay against the fresh +mrb_state+ before per-invocation
    # source / entrypoint resolution. Each +Source+ entry's +name+ is its
    # canonical identity — the filename baked into the loaded IREP's
    # +debug_info+ that surfaces in every backtrace frame originating
    # from the snippet as +(snippet:Name):line+. Duplicate names within
    # the +code:+ form would produce ambiguous attribution and are
    # rejected at registration time
    # ({docs/behavior.md E-33}[link:../../../docs/behavior.md]).
    #
    # Sealing (B-33) is governed by the owning Sandbox — the table itself
    # is append-only and exposes no mutation API beyond +#register+; the
    # Sandbox guards +#register+ behind the seal check before delegating.
    class Table
      # Ruby constant-name pattern enforced on snippet names
      # ({docs/behavior.md E-34}[link:../../../docs/behavior.md]).
      NAME_PATTERN = /\A[A-Z]\w*\z/

      # The +kind+ field value carried by source snippets in their wire
      # envelope entry
      # ({docs/wire-codec.md Invocation channels}[link:../../../docs/wire-codec.md]).
      SOURCE_KIND = "source"

      def initialize
        @entries = [] # : Array[Kobako::Snippet::Source]
      end

      # Serialize the registered snippets to wire bytes. Each entry
      # contributes a +{name, kind, body}+ map under a single msgpack
      # collection; an empty table serializes to an empty collection,
      # never absent. The wire codec is an implementation detail —
      # callers receive a binary +String+ that the +Kobako::Wasm+ layer
      # ships through the invocation channel.
      def encode
        payload = @entries.map do |entry|
          { "name" => entry.name.to_s, "kind" => SOURCE_KIND, "body" => entry.body }
        end
        MessagePack.pack(payload)
      end

      # Register +code+ under the canonical Symbol form of +name+. +code+
      # is the mruby source as a String; the bytes are re-encoded as
      # UTF-8 and detached from the caller's reference. +name+ is a
      # Symbol or String matching +NAME_PATTERN+. Returns the Symbol
      # form of +name+.
      #
      # Raises +ArgumentError+ when +name+ is malformed (E-34) or
      # duplicates an already-registered source snippet (E-33).
      def register(code, name)
        name_sym = normalize_name(name)
        raise ArgumentError, "snippet #{name_sym.inspect} already preloaded" if names.include?(name_sym)

        @entries << Source.new(name: name_sym, body: code.dup.force_encoding(Encoding::UTF_8))
        name_sym
      end

      # Iterate over registered entries in insertion order. Yields each
      # +Kobako::Snippet::Source+ instance. Returns an Enumerator when no
      # block is given.
      def each(&)
        @entries.each(&)
      end

      # All registered snippet names, in insertion order.
      def names
        @entries.map(&:name)
      end

      # Number of registered snippets.
      def size
        @entries.size
      end

      # Whether no snippets are registered.
      def empty?
        @entries.empty?
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
end
