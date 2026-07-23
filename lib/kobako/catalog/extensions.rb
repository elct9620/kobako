# frozen_string_literal: true

require_relative "../unresolved"

module Kobako
  module Catalog
    # Kobako::Catalog::Extensions — per-Sandbox registry of installed
    # Extensions. Composes each Extension onto the sibling registries at
    # install time (its +source+ into +Catalog::Snippets+, its +backend+
    # path into +Catalog::Services+), asserts declared dependencies are
    # present when the Sandbox seals, and resolves each callable-backed
    # path to a fresh object for every invocation — returning that map to the
    # driving +Context+ rather than mutating the shared registry, so
    # concurrent invocations stay shared-nothing.
    #
    # Sealing and the reject-after-install guard are governed by the owning
    # Sandbox through +Catalog::Services#sealed?+, the shared seal signal;
    # this registry only records entries and enforces the Extension-shape
    # and dependency rules.
    class Extensions
      def initialize
        @entries = [] # : Array[untyped]
        @asserted = false
      end

      # Install +extension+: validate its shape, register its +source+ as a
      # preloaded snippet, and — when it carries a +backend+ — reserve the
      # backend path in +services+. A static +object:+ binds directly; a
      # per-invocation +provider:+ or a fillable backend reserves the path with
      # +Kobako::Unresolved+ (the per-invocation result is layered onto the
      # Context, a fillable stays Unresolved until filled). Raises
      # +ArgumentError+ for the two shapes +#validate!+ owns — +source+ not a
      # String, or a present +backend+ that omits +path+ / +object+ /
      # +provider+. The Extension readers are duck-typed, so an object missing
      # them surfaces the underlying +NoMethodError+, and a malformed +name+ or
      # +backend.path+ surfaces through the +snippets+ / +services+
      # registration it routes to.
      def install(extension, snippets:, services:)
        validate!(extension)
        snippets.register(code: extension.source, name: extension.name)
        backend = extension.backend
        services.bind(backend.path, install_object(backend)) if backend
        @entries << extension
        self
      end

      # Assert every installed Extension's +depends_on+ names a fellow
      # installed Extension. Runs once, when the Sandbox first seals
      # successfully; an unmet dependency raises +ArgumentError+ naming the
      # gap, before the guest runs. The asserted flag flips only on success,
      # so a seal that failed re-checks on the next attempt rather than
      # silently passing a broken Sandbox. Idempotent across later invocations.
      def seal!
        return self if @asserted

        assert_dependencies!
        @asserted = true
        self
      end

      # Resolve each +provider:+-backed path to this invocation's object and
      # return the +path => object+ map the driving +Context+ layers over the
      # static base bindings. Distinct providers yield distinct objects; one
      # provider shared by several Extensions is invoked once and its result
      # shared, so provider identity is resource identity. Static and fillable
      # backends are omitted — they stay the object bound at install in the
      # base registry (+Kobako::Unresolved+ for a fillable).
      def resolve
        resolved = {} # : Hash[String, untyped]
        by_provider = {} # : Hash[untyped, untyped]
        by_provider.compare_by_identity
        @entries.each { |extension| resolve_backend(extension, resolved, by_provider) }
        resolved
      end

      private

      # The object a backend binds at install: a static +object:+ directly, or
      # +Kobako::Unresolved+ as the placeholder for a per-invocation +provider:+
      # (layered onto the Context) or a fillable (neither keyword).
      def install_object(backend) = backend.object.nil? ? Kobako::Unresolved : backend.object

      # Resolve one Extension's +provider:+-backed path against the shared
      # per-invocation identity cache (keyed by provider identity) and record
      # it in +resolved+. A static or fillable backend is skipped — its object
      # stays as bound at install in the base registry.
      def resolve_backend(extension, resolved, by_provider)
        backend = extension.backend
        return unless backend

        provider = backend.provider
        return if provider.nil?

        resolved[backend.path.to_s] = by_provider.fetch(provider) { by_provider[provider] = provider.call }
      end

      # Enforce the Extension-shape checks +#preload+ / +#bind+ do not: a
      # mandatory String +source+ (the install/bind boundary) and a
      # +backend+ that, when present, exposes +path+ / +object+ / +provider+.
      def validate!(extension)
        source = extension.source
        raise ArgumentError, "Extension #source must be a String, got #{source.class}" unless source.is_a?(String)

        backend = extension.backend
        return if backend.nil?
        return if backend.respond_to?(:path) && backend.respond_to?(:object) && backend.respond_to?(:provider)

        raise ArgumentError, "Extension #backend must expose #path, #object, and #provider"
      end

      def assert_dependencies!
        names = @entries.map { |extension| symbolize(extension.name) }
        @entries.each do |extension|
          (extension.depends_on || []).each do |dependency|
            next if names.include?(symbolize(dependency))

            raise ArgumentError,
                  "Extension #{extension.name.inspect} depends on #{dependency.inspect}, which is not installed"
          end
        end
      end

      # Match names and dependencies by Symbol so the Symbol-or-String forms
      # of a constant token are interchangeable; a value that is neither
      # falls through unchanged to the not-installed path.
      def symbolize(name) = name.is_a?(String) ? name.to_sym : name
    end
  end
end
