# frozen_string_literal: true

# Layer 3 unit tests for the Kobako::Catalog::Services registry. Pure
# Ruby — does NOT require the native extension. Behavioural coverage that
# needs a real Sandbox wiring (seal! triggered by the first invocation)
# lives in test/e2e/sandbox/test_preload.rb; this file pins the registry
# contract.
#
# Cross-references:
#   - SPEC.md / docs/behavior/registration.md B-08 — bind a Service at a
#     constant path (1+ segments), accepts class/instance/module
#   - SPEC.md / docs/behavior/registration.md B-09 — multiple Services
#     coexist independently; siblings share a prefix
#   - SPEC.md / docs/behavior/registration.md B-11 — duplicate or
#     prefix-colliding path raises, existing binding preserved
#   - SPEC.md / docs/behavior/errors.md E-16 — malformed path segment
#   - SPEC.md / docs/behavior/errors.md E-45 — bind after the seal

require "test_helper"

module Kobako
  class CatalogServicesTest < Minitest::Test
    def setup
      @services = Kobako::Catalog::Services.new
    end

    # ---------- B-08: bind resolves; returns self for chaining ----------

    # @behavior SV-001 SV-002
    def test_bind_resolves_a_multi_segment_path_and_chains
      logger = Object.new
      def logger.info(msg) = "logged:#{msg}"

      chain = @services.bind(:"Logger::Info", logger)
      assert_same @services, chain, "bind through the registry must return self for chaining (B-08)"
      assert_same logger, @services.lookup("Logger::Info")
    end

    # @behavior SV-003
    def test_bind_resolves_a_single_segment_top_level_path
      fs = Object.new
      @services.bind("File", fs)
      assert_same fs, @services.lookup("File")
    end

    # @behavior SV-004
    def test_bind_accepts_symbol_and_string_paths
      @services.bind(:"Logger::Info", :sym)
      @services.bind("Auth::Token", :str)
      assert_equal :sym, @services.lookup("Logger::Info")
      assert_equal :str, @services.lookup("Auth::Token")
    end

    # E-16: a path with any malformed segment is rejected at bind time.
    def test_bind_rejects_a_malformed_path_segment
      ["lower::Ok", "Ok::lower", :"Has-Dash::X", "9Numeric", "A::", "::A", "A::B::"].each do |bad|
        assert_raises(ArgumentError, "malformed path #{bad.inspect} must raise (E-16)") do
          @services.bind(bad, :obj)
        end
      end
    end

    # ---------- B-08: bind accepts class / instance / module uniformly ----------

    # @behavior SV-005
    def test_bind_accepts_class_instance_and_module
      klass, instance, mod = b08_class_instance_module_triple
      @services.bind("Mixed::K", klass).bind("Mixed::I", instance).bind("Mixed::M", mod)

      assert_same klass,    @services.lookup("Mixed::K")
      assert_same instance, @services.lookup("Mixed::I")
      assert_same mod,      @services.lookup("Mixed::M")
    end

    def b08_class_instance_module_triple
      klass = Class.new { def self.ping = :klass }
      instance = Object.new
      def instance.ping = :instance
      mod = Module.new do
        module_function

        def ping = :mod
      end
      [klass, instance, mod]
    end

    # ---------- B-09: multiple Services coexist; siblings share a prefix ----------

    # @behavior SV-010
    def test_multiple_services_resolve_independently
      @services.bind("Auth::Token", "tk")
      @services.bind("Logger::Info", "lg")

      assert_equal "tk", @services.lookup("Auth::Token")
      assert_equal "lg", @services.lookup("Logger::Info")
    end

    # @behavior SV-011
    def test_sibling_paths_under_a_shared_prefix_coexist
      @services.bind("KV::Get", :get)
      @services.bind("KV::Set", :set)
      assert_equal :get, @services.lookup("KV::Get")
      assert_equal :set, @services.lookup("KV::Set")
    end

    # ---------- B-11: duplicate / prefix collision raises ----------

    # @behavior SV-012 SV-015
    def test_bind_rejects_an_exact_duplicate_path
      @services.bind("KV::Get", :first)
      assert_raises(ArgumentError) { @services.bind("KV::Get", :second) }
      assert_equal :first, @services.lookup("KV::Get"), "the existing binding must be preserved"
    end

    # @behavior SV-013 SV-015
    def test_bind_rejects_a_path_that_extends_an_existing_leaf
      @services.bind("KV", :leaf)
      assert_raises(ArgumentError) { @services.bind("KV::Get", :under) }
      assert_equal :leaf, @services.lookup("KV"),
                   "a rejected prefix-extending bind must leave the existing leaf binding intact (B-11)"
    end

    # @behavior SV-014 SV-015
    def test_bind_rejects_a_path_that_is_a_prefix_of_an_existing_binding
      @services.bind("KV::Get", :under)
      assert_raises(ArgumentError) { @services.bind("KV", :leaf) }
      assert_equal :under, @services.lookup("KV::Get"),
                   "a rejected prefix-of-existing bind must leave the existing deeper binding intact (B-11)"
    end

    # ---------- seal / lookup error paths ----------

    # E-45: bind raises ArgumentError once Services#seal! has fired.
    def test_bind_after_seal_raises
      @services.bind("Early::A", :a)
      @services.seal!
      err = assert_raises(ArgumentError) { @services.bind("Late::B", :b) }
      assert_match(/after first Sandbox invocation/, err.message)
    end

    def test_lookup_raises_key_error_for_an_unbound_path
      @services.bind("Logger::Info", :v)
      err = assert_raises(KeyError) { @services.lookup("Logger::Missing") }
      assert_match(/Logger::Missing/, err.message)
    end
  end

  # The declared path set every invocation ships on Frame 1
  # (docs/behavior/lifecycle.md B-02), including the B-33 sealing snapshot
  # — every invocation after the seal ships the bindings that existed at
  # that moment.
  class CatalogServicesPathsTest < Minitest::Test
    def setup
      @services = Kobako::Catalog::Services.new
    end

    def test_paths_lists_every_bound_path_in_bind_order
      @services.bind("MyService::KV", :kv).bind("MyService::Logger", :log)
      @services.bind("File", :fs)

      assert_equal %w[MyService::KV MyService::Logger File], @services.paths,
                   "a bound registry through #paths must list every path in bind order"
    end

    def test_paths_on_an_empty_registry_is_the_empty_list
      assert_empty @services.paths,
                   "a registry with no bindings through #paths must be empty, never absent"
    end

    def test_paths_before_seal_reflects_new_bindings
      @services.bind("MyService::KV", :kv)
      first = @services.paths
      @services.bind("MyService::Logger", :log)

      assert_equal %w[MyService::KV], first
      assert_equal %w[MyService::KV MyService::Logger], @services.paths,
                   "binding a Service on an unsealed registry must surface in the next #paths read (B-08)"
    end

    # B-33 seals Service registration (B-08) at the first invocation.
    # Binding past the seal raises (E-45), so the declared path set is
    # stable by construction.
    def test_paths_after_seal_excludes_paths_bound_later
      @services.bind("MyService::KV", :kv)
      @services.seal!

      assert_raises(ArgumentError) { @services.bind("MyService::Late", :late) }

      assert_equal %w[MyService::KV], @services.paths,
                   "a bind rejected after the seal must not alter the declared path set (B-33 / E-45)"
    end
  end
end
