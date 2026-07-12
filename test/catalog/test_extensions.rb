# frozen_string_literal: true

# Layer 3 unit tests for the Kobako::Catalog::Extensions registry. Pure
# Ruby — does NOT require the native extension: it drives the registry
# against real Catalog::Snippets / Catalog::Services collaborators. The
# guest-observable end of the mechanism (File.join local, File.read
# dispatched) lives in the #install E2E; this file pins the host-side
# composition, provider resolution, and dependency contract.
#
# Cross-references:
#   - SPEC.md / docs/behavior/extension.md B-55 — install composes an
#     Extension into a preloaded snippet plus an optional bound backend
#   - SPEC.md / docs/behavior/extension.md B-56 — a backend's bound object
#     is fixed, or resolved fresh per invocation from a callable provider;
#     provider identity is resource identity
#   - SPEC.md / docs/behavior/extension.md B-57 — depends_on is asserted
#     for presence at the seal; cycles permitted
#   - SPEC.md / docs/behavior/errors.md E-52 — an unmet dependency raises
#   - SPEC.md / docs/behavior/errors.md E-53 — a malformed Extension raises

require "test_helper"

module Kobako
  # Shared registry fixtures + Extension builders for the Extensions suites.
  module ExtensionTestSupport
    private

    def setup_registries
      @extensions = Kobako::Catalog::Extensions.new
      @snippets = Kobako::Catalog::Snippets.new
      @services = Kobako::Catalog::Services.new
    end

    def install(extension) = @extensions.install(extension, snippets: @snippets, services: @services)

    def extension(name:, source:, backend: nil, depends_on: [])
      Kobako::Extension.new(name: name, source: source, backend: backend, depends_on: depends_on)
    end

    def backend(path, provider) = Kobako::Extension::Backend.new(path: path, provider: provider)

    # A provider that appends a fresh object per call, so a test reads the
    # invocation count off +sink.size+ and object identity off +sink.last+.
    def counting_provider(sink)
      lambda do
        sink << Object.new
        sink.last
      end
    end

    def snippet_names = MessagePack.unpack(@snippets.encode).map { |entry| entry["name"] }
  end

  # B-55 composition, B-57 / E-52 dependency presence, E-53 malformed shape.
  class CatalogExtensionsTest < Minitest::Test
    include ExtensionTestSupport

    def setup = setup_registries

    def test_install_registers_source_as_a_snippet_and_binds_the_backend_path
      fs = Object.new
      install(extension(name: :File, source: "class File < Kobako::Member; end", backend: backend("Vfs", fs)))

      assert_equal ["File"], snippet_names,
                   "install must register the Extension source as a snippet named by #name (B-55)"
      assert_same fs, @services.lookup("Vfs"),
                  "install must bind the backend at backend.path, independent of #name (B-55)"
    end

    def test_install_of_a_pure_guest_extension_binds_no_service
      install(extension(name: :Errno, source: "module Errno; end"))

      assert_equal ["Errno"], snippet_names
      assert_equal [], MessagePack.unpack(@services.encode),
                   "a pure-guest Extension (no backend) must bind no Service (B-55)"
    end

    def test_install_returns_self_for_chaining
      assert_same @extensions, install(extension(name: :A, source: "1"))
    end

    def test_seal_accepts_satisfied_dependencies
      install(extension(name: :Errno, source: "1"))
      install(extension(name: :File, source: "2", depends_on: [:Errno]))

      assert_same @extensions, @extensions.seal!
    end

    # E-52: an unmet dependency raises at the seal, naming both ends.
    def test_seal_raises_on_an_unmet_dependency_naming_it
      install(extension(name: :File, source: "1", depends_on: [:Errno]))

      err = assert_raises(ArgumentError) { @extensions.seal! }
      assert_match(/:File/, err.message)
      assert_match(/:Errno/, err.message,
                   "an unmet dependency assertion names the missing Extension (B-57 / E-52)")
    end

    def test_seal_permits_dependency_cycles
      install(extension(name: :A, source: "1", depends_on: [:B]))
      install(extension(name: :B, source: "2", depends_on: [:A]))

      assert_same @extensions, @extensions.seal!,
                  "presence-only assertion permits dependency cycles (B-57)"
    end

    # The dependency assertion is a one-time gate at the first seal, not a
    # per-invocation check — begin_invocation! calls seal! on every
    # invocation and relies on it staying silent afterward. Drive the
    # registry directly (the Sandbox refuses install once sealed) to add an
    # unmet dependency after the first seal: a second seal must neither
    # re-assert nor raise.
    def test_seal_asserts_dependencies_once_then_is_a_silent_no_op
      install(extension(name: :A, source: "1"))
      assert_same @extensions, @extensions.seal!

      install(extension(name: :B, source: "2", depends_on: [:Missing]))
      assert_same @extensions, @extensions.seal!,
                  "seal! asserts dependencies only at the first seal, so a dependency left " \
                  "unmet afterward does not raise on a later seal (B-57)"
    end

    def test_install_rejects_a_non_string_source
      err = assert_raises(ArgumentError) { install(extension(name: :File, source: 123)) }
      assert_match(/source/, err.message, "a non-String source is a malformed Extension (E-53)")
    end

    def test_install_rejects_a_backend_missing_path_or_provider
      err = assert_raises(ArgumentError) { install(extension(name: :File, source: "1", backend: Object.new)) }
      assert_match(/backend/, err.message,
                   "a backend that does not expose #path and #provider is malformed (E-53)")
    end
  end

  # B-56 backend provider resolution: fixed vs per-invocation, identity.
  class CatalogExtensionsProviderTest < Minitest::Test
    include ExtensionTestSupport

    def setup = setup_registries

    def test_fixed_backend_is_bound_at_install_and_stays_one_object
      fs = Object.new
      install(extension(name: :File, source: "1", backend: backend("File", fs)))

      assert_same fs, @services.lookup("File"), "a fixed provider is bound directly at install (B-55)"
      @extensions.refresh_backends!(@services)
      assert_same fs, @services.lookup("File"),
                  "a fixed provider stays the same object across invocations (B-56)"
    end

    def test_callable_backend_resolves_a_fresh_object_each_invocation
      install(extension(name: :File, source: "1", backend: backend("File", -> { Object.new })))
      assert_nil @services.lookup("File"), "a callable backend holds a placeholder until refresh (B-56)"

      @extensions.refresh_backends!(@services)
      first = @services.lookup("File")
      @extensions.refresh_backends!(@services)

      refute_same first, @services.lookup("File"), "each invocation resolves a fresh backend object (B-56)"
    end

    def test_one_provider_shared_by_several_extensions_resolves_once_per_invocation
      sink = []
      shared = counting_provider(sink)
      install(extension(name: :File, source: "1", backend: backend("File", shared)))
      install(extension(name: :Dir, source: "2", backend: backend("Dir", shared)))

      @extensions.refresh_backends!(@services)

      assert_equal 1, sink.size, "one provider shared by several Extensions resolves once per invocation (B-56)"
      assert_same @services.lookup("File"), @services.lookup("Dir"),
                  "a shared provider must back every path with the same object (B-56)"
    end

    def test_distinct_providers_resolve_to_distinct_objects
      install(extension(name: :File, source: "1", backend: backend("File", -> { Object.new })))
      install(extension(name: :Dir, source: "2", backend: backend("Dir", -> { Object.new })))

      @extensions.refresh_backends!(@services)

      refute_same @services.lookup("File"), @services.lookup("Dir"),
                  "distinct providers must resolve to distinct objects (B-56)"
    end
  end
end
