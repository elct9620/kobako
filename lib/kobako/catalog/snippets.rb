# frozen_string_literal: true

require_relative "../constant_name"
require_relative "../snippet"

module Kobako
  module Catalog
    class Snippets # :nodoc:
      def initialize
        @entries = [] # : Array[Kobako::Snippet::Source | Kobako::Snippet::Binary]
      end

      # Projected here so the entry value objects stay pure carriers.
      def entries
        @entries.map { |entry| entry_tuple(entry) }
      end

      # Bytecode stays unchecked on the host; the guest validates it at
      # first replay.
      def register(code: nil, name: nil, binary: nil)
        if binary
          raise ArgumentError, "cannot combine binary: with code: / name:" if code || name

          register_binary!(binary)
        else
          register_source!(code, name)
        end
      end

      private

      # The native side frames a source body only as UTF-8 text, so source
      # read as bytes is re-tagged rather than refused.
      def register_source!(code, name)
        code, name = ensure_source_args!(code, name)
        name_sym = normalize_name(name)
        if @entries.any? { |e| e.is_a?(Snippet::Source) && e.name == name_sym }
          raise ArgumentError, "snippet #{name_sym.inspect} already preloaded"
        end

        @entries << Snippet::Source.new(name: name_sym, body: code.dup.force_encoding(Encoding::UTF_8))
        nil
      end

      # The +code:+ type check runs first so an explicit +code: nil+ reads
      # as a type error rather than a missing keyword.
      def ensure_source_args!(code, name)
        raise ArgumentError, "missing keyword: code: + name:, or binary:" if code.nil? && name.nil?
        raise ArgumentError, "code must be a String, got #{code.class}" unless code.is_a?(String)
        raise ArgumentError, "missing keyword: name:" if name.nil?

        [code, name]
      end

      # Forced to ASCII-8BIT so msgpack-ruby picks the +bin+ family on the
      # wire.
      def register_binary!(bytes)
        raise ArgumentError, "binary must be a String, got #{bytes.class}" unless bytes.is_a?(String)

        @entries << Snippet::Binary.new(body: bytes.dup.force_encoding(Encoding::ASCII_8BIT))
        nil
      end

      # A bytecode entry carries no name: its canonical name lives in the
      # bytecode's +debug_info+ and is read by the guest at load time.
      def entry_tuple(entry)
        case entry
        when Snippet::Source
          [Snippet::Source::KIND, entry.name.to_s, entry.body]
        else
          [Snippet::Binary::KIND, nil, entry.body]
        end
      end

      def normalize_name(name)
        unless name.is_a?(Symbol) || name.is_a?(String)
          raise ArgumentError, "snippet name must be a Symbol or String, got #{name.class}"
        end

        name_str = name.to_s
        unless CONSTANT_NAME.match?(name_str)
          raise ArgumentError,
                "snippet name must match #{CONSTANT_NAME.inspect} (got #{name.inspect})"
        end

        name_str.to_sym
      end
    end
  end
end
