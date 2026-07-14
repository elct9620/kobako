# frozen_string_literal: true

module Kobako
  module Catalog
    # Kobako::Catalog::Extensions — per-Sandbox registry of installed
    # Extensions. Composes each Extension onto the sibling registries at
    # install time (its +source+ into +Catalog::Snippets+, its +backend+
    # path into +Catalog::Services+), asserts declared dependencies are
    # present when the Sandbox seals, and resolves each callable-backed
    # path to a fresh object at the start of every invocation.
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
      # backend path in +services+. A callable provider reserves the path
      # with a placeholder the per-invocation refresh replaces; a fixed
      # provider binds its object directly. Raises +ArgumentError+ for the two
      # shapes +#validate!+ owns — +source+ not a String, or a present
      # +backend+ that omits +path+ / +provider+. The Extension readers are
      # duck-typed, so an object missing them surfaces the underlying
      # +NoMethodError+, and a malformed +name+ or +backend.path+ surfaces
      # through the +snippets+ / +services+ registration it routes to.
      def install(extension, snippets:, services:)
        validate!(extension)
        snippets.register(code: extension.source, name: extension.name)
        backend = extension.backend
        services.bind(backend.path, initial_object(backend.provider)) if backend
        @entries << extension
        self
      end

      # Assert every installed Extension's +depends_on+ names a fellow
      # installed Extension. Runs once, when the Sandbox first seals; an
      # unmet dependency raises +ArgumentError+ naming the gap, before the
      # guest runs. Idempotent across later invocations.
      def seal!
        return self if @asserted

        @asserted = true
        assert_dependencies!
        self
      end

      # Resolve each callable-backed path to this invocation's object and
      # refresh it behind its already-sealed path in +services+. Distinct
      # providers yield distinct objects; one provider shared by several
      # Extensions is invoked once and its result shared, so provider
      # identity is resource identity. Fixed providers are left untouched —
      # they stay the object bound at install.
      def refresh_backends!(services)
        resolved = {} # : Hash[untyped, untyped]
        resolved.compare_by_identity
        @entries.each { |extension| refresh_backend(extension, services, resolved) }
        self
      end

      private

      # Resolve one Extension's callable-backed path against the shared
      # per-invocation +resolved+ cache (keyed by provider identity) and
      # refresh it in +services+. A fixed provider is skipped — its object
      # stays as bound at install.
      def refresh_backend(extension, services, resolved)
        backend = extension.backend
        return unless backend

        provider = backend.provider
        return unless callable?(provider)

        object = resolved.fetch(provider) { resolved[provider] = provider.call }
        services.refresh(backend.path, object)
      end

      # Enforce the Extension-shape checks +#preload+ / +#bind+ do not: a
      # mandatory String +source+ (the install/bind boundary) and a
      # +backend+ that, when present, exposes +path+ and +provider+.
      def validate!(extension)
        source = extension.source
        raise ArgumentError, "Extension #source must be a String, got #{source.class}" unless source.is_a?(String)

        backend = extension.backend
        return if backend.nil?
        return if backend.respond_to?(:path) && backend.respond_to?(:provider)

        raise ArgumentError, "Extension #backend must expose #path and #provider"
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

      def callable?(provider) = provider.respond_to?(:call)

      # A callable provider reserves its path with a placeholder that the
      # per-invocation refresh replaces before any dispatch; a fixed
      # provider is the bound object itself.
      def initial_object(provider) = callable?(provider) ? nil : provider
    end
  end
end
