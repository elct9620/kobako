# frozen_string_literal: true

# Layer 3 unit tests for the Kobako::Catalog::Namespaces registry. Pure
# Ruby — does NOT require the native extension. Behavioural coverage that
# needs a real Sandbox wiring (seal! triggered by the first invocation)
# lives in test/test_sandbox_preload.rb; this file pins the registry
# contract. The Kobako::Namespace entity is covered in
# test/test_namespace.rb.
#
# Cross-references:
#   - SPEC.md / docs/behavior.md B-07 — Namespace declaration + name validation
#   - SPEC.md / docs/behavior.md B-08 — Member binding accepts class/instance/module
#   - SPEC.md / docs/behavior.md B-09 — Multiple Namespaces coexist independently
#   - SPEC.md / docs/behavior.md B-10 — define is idempotent
#   - SPEC.md / docs/behavior.md B-11 — Duplicate bind raises, existing binding preserved
#   - SPEC.md / docs/behavior.md E-16 — Malformed Namespace name
#   - SPEC.md / docs/behavior.md E-17 — Malformed Member name

require "minitest/autorun"
require "msgpack"

$LOAD_PATH.unshift File.expand_path("../../lib", __dir__)
require "kobako/catalog/namespaces"

module Kobako
  class CatalogNamespacesTest < Minitest::Test
    def setup
      @namespaces = Kobako::Catalog::Namespaces.new
    end

    # ---------- B-07 / B-10: define returns Namespace; idempotent ----------

    def test_define_returns_namespace_and_lookup_resolves_bound_member
      logger = Object.new
      def logger.info(msg) = "logged:#{msg}"

      namespace = @namespaces.define(:Logger)
      assert_instance_of Namespace, namespace

      chain = namespace.bind(:Info, logger)
      assert_same namespace, chain, "bind must return self for chaining (B-08)"
      assert_same logger, @namespaces.lookup("Logger::Info")
    end

    def test_define_is_idempotent_returning_same_namespace_instance
      first = @namespaces.define(:Auth)
      first.bind(:Token, :original)

      second = @namespaces.define(:Auth)
      assert_same first, second
      assert_equal :original, @namespaces.lookup("Auth::Token")
    end

    def test_define_accepts_string_form
      namespace = @namespaces.define("Logger")
      assert_equal "Logger", namespace.name
      namespace.bind("Info", :v)
      assert_equal :v, @namespaces.lookup("Logger::Info")
    end

    # E-16: malformed Namespace names rejected at #define time.
    def test_define_rejects_malformed_namespace_name
      [:lower, :"Has-Dash", "9Numeric"].each do |bad|
        assert_raises(ArgumentError) { @namespaces.define(bad) }
      end
    end

    # B-07 Notes: define raises once Namespaces#seal! has fired. This is the
    # mechanism Sandbox's first invocation rides on; the Sandbox-surface
    # observable lives in test_sandbox_preload.rb.
    def test_define_after_seal_raises
      @namespaces.define(:Early)
      @namespaces.seal!
      err = assert_raises(ArgumentError) { @namespaces.define(:Late) }
      assert_match(/after first Sandbox invocation/, err.message)
    end

    # ---------- B-08: bind accepts class / instance / module uniformly ----------

    def test_namespace_bind_accepts_class_instance_and_module
      klass, instance, mod = b08_class_instance_module_triple
      @namespaces.define(:Mixed).bind(:K, klass).bind(:I, instance).bind(:M, mod)

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

    # ---------- B-09: multiple namespaces coexist; independent lookup ----------

    def test_multiple_namespaces_resolve_independently
      @namespaces.define(:Auth).bind(:Token, "tk")
      @namespaces.define(:Logger).bind(:Info, "lg")

      assert_equal "tk", @namespaces.lookup("Auth::Token")
      assert_equal "lg", @namespaces.lookup("Logger::Info")
    end

    # ---------- lookup error paths ----------

    def test_lookup_raises_key_error_for_unknown_member
      @namespaces.define(:Logger) # no members bound
      err = assert_raises(KeyError) { @namespaces.lookup("Logger::Missing") }
      assert_match(/Missing/, err.message)
    end

    def test_lookup_raises_key_error_for_unknown_namespace
      err = assert_raises(KeyError) { @namespaces.lookup("Ghost::Member") }
      assert_match(/Ghost/, err.message)
    end

    # ---------- to_preamble / encode (Frame 1 wire shape) ----------

    def test_encoded_preamble_decodes_to_two_level_array_of_namespace_descriptors
      @namespaces.define(:MyService).bind(:KV, :kv).bind(:Logger, :log)
      @namespaces.define(:Auth).bind(:Token, :tk)

      bytes = @namespaces.encode
      assert_kind_of String, bytes
      assert_equal Encoding::ASCII_8BIT, bytes.encoding

      decoded = MessagePack.unpack(bytes)
      assert_equal [["MyService", %w[KV Logger]], ["Auth", %w[Token]]], decoded
    end

    def test_encoded_preamble_empty_registry_is_valid_msgpack_array
      decoded = MessagePack.unpack(@namespaces.encode)
      assert_equal [], decoded
    end

    def test_encoded_preamble_with_only_empty_namespace_emits_empty_member_list
      @namespaces.define(:Empty)
      decoded = MessagePack.unpack(@namespaces.encode)
      assert_equal [["Empty", []]], decoded
    end
  end
end
