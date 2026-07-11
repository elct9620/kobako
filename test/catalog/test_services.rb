# frozen_string_literal: true

# Layer 3 unit tests for the Kobako::Catalog::Services registry. Pure
# Ruby — does NOT require the native extension. Behavioural coverage that
# needs a real Sandbox wiring (seal! triggered by the first invocation)
# lives in test/sandbox/test_preload.rb; this file pins the registry
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
      @namespaces = Kobako::Catalog::Services.new
    end

    # ---------- B-08: bind resolves; returns self for chaining ----------

    def test_bind_resolves_a_multi_segment_path_and_chains
      logger = Object.new
      def logger.info(msg) = "logged:#{msg}"

      chain = @namespaces.bind(:"Logger::Info", logger)
      assert_same @namespaces, chain, "bind through the registry must return self for chaining (B-08)"
      assert_same logger, @namespaces.lookup("Logger::Info")
    end

    def test_bind_resolves_a_single_segment_top_level_path
      fs = Object.new
      @namespaces.bind("File", fs)
      assert_same fs, @namespaces.lookup("File")
    end

    def test_bind_accepts_symbol_and_string_paths
      @namespaces.bind(:"Logger::Info", :sym)
      @namespaces.bind("Auth::Token", :str)
      assert_equal :sym, @namespaces.lookup("Logger::Info")
      assert_equal :str, @namespaces.lookup("Auth::Token")
    end

    # E-16: a path with any malformed segment is rejected at bind time.
    def test_bind_rejects_a_malformed_path_segment
      ["lower::Ok", "Ok::lower", :"Has-Dash::X", "9Numeric", "A::", "::A", "A::B::"].each do |bad|
        assert_raises(ArgumentError, "malformed path #{bad.inspect} must raise (E-16)") do
          @namespaces.bind(bad, :obj)
        end
      end
    end

    # ---------- B-08: bind accepts class / instance / module uniformly ----------

    def test_bind_accepts_class_instance_and_module
      klass, instance, mod = b08_class_instance_module_triple
      @namespaces.bind("Mixed::K", klass).bind("Mixed::I", instance).bind("Mixed::M", mod)

      assert_same klass,    @namespaces.lookup("Mixed::K")
      assert_same instance, @namespaces.lookup("Mixed::I")
      assert_same mod,      @namespaces.lookup("Mixed::M")
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

    def test_multiple_services_resolve_independently
      @namespaces.bind("Auth::Token", "tk")
      @namespaces.bind("Logger::Info", "lg")

      assert_equal "tk", @namespaces.lookup("Auth::Token")
      assert_equal "lg", @namespaces.lookup("Logger::Info")
    end

    def test_sibling_paths_under_a_shared_prefix_coexist
      @namespaces.bind("KV::Get", :get)
      @namespaces.bind("KV::Set", :set)
      assert_equal :get, @namespaces.lookup("KV::Get")
      assert_equal :set, @namespaces.lookup("KV::Set")
    end

    # ---------- B-11: duplicate / prefix collision raises ----------

    def test_bind_rejects_an_exact_duplicate_path
      @namespaces.bind("KV::Get", :first)
      assert_raises(ArgumentError) { @namespaces.bind("KV::Get", :second) }
      assert_equal :first, @namespaces.lookup("KV::Get"), "the existing binding must be preserved"
    end

    def test_bind_rejects_a_path_that_extends_an_existing_leaf
      @namespaces.bind("KV", :leaf)
      assert_raises(ArgumentError) { @namespaces.bind("KV::Get", :under) }
    end

    def test_bind_rejects_a_path_that_is_a_prefix_of_an_existing_binding
      @namespaces.bind("KV::Get", :under)
      assert_raises(ArgumentError) { @namespaces.bind("KV", :leaf) }
    end

    # ---------- seal / lookup error paths ----------

    # E-45: bind raises ArgumentError once Namespaces#seal! has fired.
    def test_bind_after_seal_raises
      @namespaces.bind("Early::A", :a)
      @namespaces.seal!
      err = assert_raises(ArgumentError) { @namespaces.bind("Late::B", :b) }
      assert_match(/after first Sandbox invocation/, err.message)
    end

    def test_lookup_raises_key_error_for_an_unbound_path
      @namespaces.bind("Logger::Info", :v)
      err = assert_raises(KeyError) { @namespaces.lookup("Logger::Missing") }
      assert_match(/Logger::Missing/, err.message)
    end
  end

  # Frame 1 wire shape: the flat preamble emitted by Namespaces#encode
  # (docs/behavior/lifecycle.md B-02), including the B-33 sealing snapshot
  # — every invocation after the seal ships the bindings that existed at
  # that moment.
  class CatalogServicesPreambleTest < Minitest::Test
    def setup
      @namespaces = Kobako::Catalog::Services.new
    end

    def test_encoded_preamble_decodes_to_a_flat_array_of_bind_paths
      @namespaces.bind("MyService::KV", :kv).bind("MyService::Logger", :log)
      @namespaces.bind("File", :fs)

      bytes = @namespaces.encode
      assert_kind_of String, bytes
      assert_equal Encoding::ASCII_8BIT, bytes.encoding

      decoded = MessagePack.unpack(bytes)
      assert_equal %w[MyService::KV MyService::Logger File], decoded
    end

    def test_encoded_preamble_empty_registry_is_valid_msgpack_array
      decoded = MessagePack.unpack(@namespaces.encode)
      assert_equal [], decoded
    end

    def test_encoded_preamble_before_seal_reflects_new_bindings
      @namespaces.bind("MyService::KV", :kv)
      first = MessagePack.unpack(@namespaces.encode)
      @namespaces.bind("MyService::Logger", :log)

      assert_equal %w[MyService::KV], first
      assert_equal %w[MyService::KV MyService::Logger], MessagePack.unpack(@namespaces.encode),
                   "binding a Service on an unsealed registry must surface in the next Frame 1 encode (B-08)"
    end

    # B-33 seals Service registration (B-08) at the first invocation.
    # Binding past the seal raises (E-45), so the sealed Frame 1 preamble
    # is stable by construction.
    def test_encoded_preamble_after_seal_excludes_paths_bound_later
      @namespaces.bind("MyService::KV", :kv)
      @namespaces.seal!
      sealed_bytes = @namespaces.encode

      assert_raises(ArgumentError) { @namespaces.bind("MyService::Late", :late) }

      assert_equal sealed_bytes, @namespaces.encode,
                   "a bind rejected after the seal must not alter the Frame 1 preamble (B-33 / E-45)"
      assert_equal %w[MyService::KV], MessagePack.unpack(@namespaces.encode)
    end
  end
end
