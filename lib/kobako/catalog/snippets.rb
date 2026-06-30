# frozen_string_literal: true

require_relative "../codec"
require_relative "../snippet"

module Kobako
  module Catalog
    # Kobako::Catalog::Snippets — per-Sandbox insertion-ordered registry
    # of preloaded snippets.
    #
    # Entries replay against the fresh +mrb_state+ before per-invocation
    # source / entrypoint resolution. Each +Snippet::Source+ entry's +name+
    # is its canonical identity — the filename baked into the loaded IREP's
    # +debug_info+ that surfaces in every backtrace frame originating from
    # the snippet as +(snippet:Name):line+. Duplicate names within the
    # +code:+ form would produce ambiguous attribution and are rejected at
    # registration time.
    # +Snippet::Binary+ entries carry no host-side name — their canonical
    # name lives in the bytecode's +debug_info+ and is read by the guest at
    # load time; the host does not extract it.
    #
    # Sealing is governed by the owning Sandbox — the registry itself
    # is append-only and exposes no mutation API beyond +#register+; the
    # Sandbox guards +#register+ behind the seal check before delegating.
    class Snippets
      # Ruby constant-name pattern enforced on snippet names.
      NAME_PATTERN = /\A[A-Z]\w*\z/

      def initialize
        @entries = [] # : Array[Kobako::Snippet::Source | Kobako::Snippet::Binary]
        @encoded = nil # : String?
      end

      # Serialize the registered snippets to wire bytes. Each entry
      # contributes a msgpack map shape; the collection rides as a single
      # msgpack array. An empty registry serializes to an empty array, never
      # absent. The wire codec is an implementation detail — callers
      # receive a binary +String+ that the +Kobako::Runtime+ layer ships
      # through the invocation channel. The entry value objects stay pure
      # carriers — this collection-tier method reads their attributes
      # externally via +entry_payload+ rather than asking each entry to
      # self-encode.
      #
      # The bytes are memoized — the table is replayed verbatim on every
      # invocation after sealing, so Frame 3 never changes between
      # encodes; {#register} drops the memo while the table is still open.
      # Unlike +Catalog::Namespaces#encode+, which gates its memo on the
      # seal, this one can fill eagerly and invalidate in +#register+
      # because every mutation funnels through that single method — there is
      # no out-of-sight child object to change the result behind its back.
      def encode
        return @encoded if @encoded

        @encoded = Codec::Encoder.encode(@entries.map { |entry| entry_payload(entry) }).freeze
      end

      # Register one preloaded snippet in either of two forms.
      #
      #   * Source form +register(code: src, name: Name)+ — +src+ is the
      #     mruby source as a String; the bytes are re-encoded as UTF-8
      #     and detached from the caller's reference. +name+ is a Symbol
      #     or String matching +NAME_PATTERN+. Returns the Symbol form
      #     of +name+.
      #   * Binary form +register(binary: bytes)+ — +bytes+ is
      #     precompiled RITE bytecode as a String, duplicated and forced
      #     to ASCII-8BIT so msgpack-ruby ships it as +bin+. Returns
      #     +nil+ — bytecode entries are anonymous on the host side; any
      #     structural validation is deferred to the guest at first replay.
      #
      # The two forms are mutually exclusive: shape validation lives
      # here so callers (chiefly +Kobako::Sandbox#preload+) collapse to
      # a single delegation. Raises +ArgumentError+ on mixed forms,
      # missing keywords, wrong types, malformed +name+, or
      # duplicate +code:+ +name+.
      def register(code: nil, name: nil, binary: nil)
        @encoded = nil
        if binary
          raise ArgumentError, "cannot combine binary: with code: / name:" if code || name

          register_binary!(binary)
        else
          register_source!(code, name)
        end
      end

      private

      # Source-form register path. Delegates argument-shape checks to
      # +ensure_source_args!+ (which returns the narrowed +[code, name]+
      # pair), normalises +name+ to a Symbol, rejects duplicates,
      # and appends the Source entry.
      def register_source!(code, name)
        code, name = ensure_source_args!(code, name)
        name_sym = normalize_name(name)
        if @entries.any? { |e| e.is_a?(Snippet::Source) && e.name == name_sym }
          raise ArgumentError, "snippet #{name_sym.inspect} already preloaded"
        end

        @entries << Snippet::Source.new(name: name_sym, body: code.dup.force_encoding(Encoding::UTF_8))
        name_sym
      end

      # Shape-only validation for the +code:+ + +name:+ pair. Returns
      # the pair with +nil+ narrowed away so callers can treat both as
      # present. The +code:+ type check runs before the +name:+
      # presence check so callers passing +code: nil+ explicitly see
      # the type error rather than the "missing keyword" error.
      def ensure_source_args!(code, name)
        raise ArgumentError, "missing keyword: code: + name:, or binary:" if code.nil? && name.nil?
        raise ArgumentError, "code must be a String, got #{code.class}" unless code.is_a?(String)
        raise ArgumentError, "missing keyword: name:" if name.nil?

        [code, name]
      end

      # Binary-form register path. Validates the +binary:+ payload type
      # and appends the Binary entry. The bytes are duplicated and forced
      # to ASCII-8BIT so msgpack-ruby picks the +bin+ family on the wire.
      def register_binary!(bytes)
        raise ArgumentError, "binary must be a String, got #{bytes.class}" unless bytes.is_a?(String)

        @entries << Snippet::Binary.new(body: bytes.dup.force_encoding(Encoding::ASCII_8BIT))
        nil
      end

      # Build the msgpack-ready Hash for one entry. Source entries
      # contribute their host-side +name+; Binary entries omit it
      # because the canonical name lives in the bytecode's embedded
      # +debug_info+ and is read by the guest at load time
      # ({docs/wire-codec.md Invocation channels}[link:../../../docs/wire-codec.md]).
      def entry_payload(entry)
        case entry
        when Snippet::Source
          { "name" => entry.name.to_s, "kind" => Snippet::Source::KIND, "body" => entry.body }
        when Snippet::Binary
          { "kind" => Snippet::Binary::KIND, "body" => entry.body }
        end
      end

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
