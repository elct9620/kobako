# frozen_string_literal: true

# Layer 3 unit tests for Kobako::Catalog::Binding and its Namespace
# child. Pure Ruby — does NOT require the native extension. Behavioural
# coverage that needs a real Sandbox wiring (seal! triggered by the
# first invocation) lives in test/test_sandbox_preload.rb; this file
# pins the Binding / Namespace contract end-to-end without involving
# Runtime.
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
require "kobako/catalog/binding"

module Kobako
  class CatalogBindingTest < Minitest::Test
    Namespace = Kobako::Catalog::Binding::Namespace

    def setup
      @binding = Kobako::Catalog::Binding.new
    end

    # ---------- B-07 / B-10: define returns Namespace; idempotent ----------

    def test_define_returns_namespace_and_lookup_resolves_bound_member
      logger = Object.new
      def logger.info(msg) = "logged:#{msg}"

      namespace = @binding.define(:Logger)
      assert_instance_of Namespace, namespace

      chain = namespace.bind(:Info, logger)
      assert_same namespace, chain, "bind must return self for chaining (B-08)"
      assert_same logger, @binding.lookup("Logger::Info")
    end

    def test_define_is_idempotent_returning_same_namespace_instance
      first = @binding.define(:Auth)
      first.bind(:Token, :original)

      second = @binding.define(:Auth)
      assert_same first, second
      assert_equal :original, @binding.lookup("Auth::Token")
    end

    def test_define_accepts_string_form
      namespace = @binding.define("Logger")
      assert_equal "Logger", namespace.name
      namespace.bind("Info", :v)
      assert_equal :v, @binding.lookup("Logger::Info")
    end

    # E-16: malformed Namespace names rejected at #define time.
    def test_define_rejects_malformed_namespace_name
      [:lower, :"Has-Dash", "9Numeric"].each do |bad|
        assert_raises(ArgumentError) { @binding.define(bad) }
      end
    end

    # B-07 Notes: define raises once Binding#seal! has fired. This is the
    # mechanism Sandbox's first invocation rides on; the Sandbox-surface
    # observable lives in test_sandbox_preload.rb.
    def test_define_after_seal_raises
      @binding.define(:Early)
      @binding.seal!
      err = assert_raises(ArgumentError) { @binding.define(:Late) }
      assert_match(/after first Sandbox invocation/, err.message)
    end

    # ---------- B-08: bind accepts class / instance / module uniformly ----------

    def test_namespace_bind_accepts_class_instance_and_module
      klass, instance, mod = b08_class_instance_module_triple
      @binding.define(:Mixed).bind(:K, klass).bind(:I, instance).bind(:M, mod)

      assert_same klass,    @binding.lookup("Mixed::K")
      assert_same instance, @binding.lookup("Mixed::I")
      assert_same mod,      @binding.lookup("Mixed::M")
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
      @binding.define(:Auth).bind(:Token, "tk")
      @binding.define(:Logger).bind(:Info, "lg")

      assert_equal "tk", @binding.lookup("Auth::Token")
      assert_equal "lg", @binding.lookup("Logger::Info")
    end

    # ---------- lookup error paths ----------

    def test_lookup_raises_key_error_for_unknown_member
      @binding.define(:Logger) # no members bound
      err = assert_raises(KeyError) { @binding.lookup("Logger::Missing") }
      assert_match(/Missing/, err.message)
    end

    def test_lookup_raises_key_error_for_unknown_namespace
      err = assert_raises(KeyError) { @binding.lookup("Ghost::Member") }
      assert_match(/Ghost/, err.message)
    end

    # ---------- to_preamble / encoded_preamble (Frame 1 wire shape) ----------

    def test_encoded_preamble_decodes_to_two_level_array_of_namespace_descriptors
      @binding.define(:MyService).bind(:KV, :kv).bind(:Logger, :log)
      @binding.define(:Auth).bind(:Token, :tk)

      bytes = @binding.encoded_preamble
      assert_kind_of String, bytes
      assert_equal Encoding::ASCII_8BIT, bytes.encoding

      decoded = MessagePack.unpack(bytes)
      assert_equal [["MyService", %w[KV Logger]], ["Auth", %w[Token]]], decoded
    end

    def test_encoded_preamble_empty_registry_is_valid_msgpack_array
      decoded = MessagePack.unpack(@binding.encoded_preamble)
      assert_equal [], decoded
    end

    def test_encoded_preamble_with_only_empty_namespace_emits_empty_member_list
      @binding.define(:Empty)
      decoded = MessagePack.unpack(@binding.encoded_preamble)
      assert_equal [["Empty", []]], decoded
    end
  end

  class CatalogBindingNamespaceEdgeCasesTest < Minitest::Test
    Klass = Kobako::Catalog::Binding::Namespace

    # ---------- bind: non-symbol/non-string member name (E-17) ----------

    # SPEC B-08 Notes / E-17: bind coerces the member name to a String via
    # #to_s before pattern-matching against the constant form. Integer 42
    # becomes "42" and fails the leading-letter check.
    def test_bind_with_integer_member_name_raises_argument_error
      namespace = Klass.new("Logger")
      err = assert_raises(ArgumentError) { namespace.bind(42, Object.new) }
      assert_match(/MemberName/, err.message)
    end

    # An Array coerces via #to_s to a String that cannot match the
    # constant pattern — another non-symbol/non-string path.
    def test_bind_with_array_member_name_raises_argument_error
      namespace = Klass.new("Logger")
      assert_raises(ArgumentError) { namespace.bind([:bad], Object.new) }
    end

    # E-17: lowercase and dashed forms reach the pattern check after #to_s.
    def test_bind_with_malformed_string_member_name_raises_argument_error
      namespace = Klass.new("Logger")
      assert_raises(ArgumentError) { namespace.bind(:lower, Object.new) }
      assert_raises(ArgumentError) { namespace.bind(:"Has-Dash", Object.new) }
    end

    # ---------- duplicate bind (B-11) ----------

    def test_duplicate_bind_at_namespace_level_raises_and_preserves_original
      namespace = Klass.new("Logger")
      namespace.bind(:Info, :first)

      err = assert_raises(ArgumentError) { namespace.bind(:Info, :second) }
      assert_match(/already bound/, err.message)
      assert_equal :first, namespace.fetch("Info"),
                   "original binding must survive duplicate-bind attempt"
    end

    # ---------- bind name normalization: Symbol vs String key ----------

    # SPEC B-08: both Symbol and String forms of a constant-pattern name
    # must be accepted and normalized to the same String key internally.
    def test_bind_with_string_member_name_normalizes_to_string_key
      namespace = Klass.new("Logger")
      namespace.bind("Info", :v)
      assert_equal :v, namespace.fetch("Info")
    end

    # ---------- fetch raises for missing member ----------

    def test_fetch_raises_key_error_for_unknown_member
      namespace = Klass.new("Logger")
      err = assert_raises(KeyError) { namespace.fetch("Unknown") }
      assert_match(/Unknown/, err.message)
    end
  end
end
