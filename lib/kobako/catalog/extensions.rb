# frozen_string_literal: true

require_relative "../unresolved"

module Kobako
  module Catalog
    # The installed Extensions, composed onto the Service and snippet
    # registries. Providers resolve afresh for each invocation and are handed
    # back rather than stored, so concurrent invocations share nothing.
    class Extensions # :nodoc:
      def initialize
        @entries = [] # : Array[untyped]
        @asserted = false
      end

      # The Extension readers are duck-typed, so a malformed +name+ or
      # +backend.path+ surfaces through the registry it routes to.
      def install(extension, snippets:, services:)
        validate!(extension)
        snippets.register(code: extension.source, name: extension.name)
        backend = extension.backend
        services.bind(backend.path, install_object(backend)) if backend
        @entries << extension
        self
      end

      # The asserted flag flips only on success, so a seal that failed
      # re-checks on the next attempt rather than silently passing a broken
      # Sandbox.
      def seal!
        return self if @asserted

        assert_dependencies!
        @asserted = true
        self
      end

      # One provider shared by several Extensions is invoked once and its
      # result shared, so provider identity is resource identity.
      def resolve
        resolved = {} # : Hash[String, untyped]
        by_provider = {} # : Hash[untyped, untyped]
        by_provider.compare_by_identity
        @entries.each { |extension| resolve_backend(extension, resolved, by_provider) }
        resolved
      end

      private

      def install_object(backend) = backend.object.nil? ? Kobako::Unresolved : backend.object

      def resolve_backend(extension, resolved, by_provider)
        backend = extension.backend
        return unless backend

        provider = backend.provider
        return if provider.nil?

        resolved[backend.path.to_s] = by_provider.fetch(provider) { by_provider[provider] = provider.call }
      end

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

      def symbolize(name) = name.is_a?(String) ? name.to_sym : name
    end
  end
end
