# frozen_string_literal: true

# Layer 3 unit tests for Kobako::RPC::Namespace edge cases.
#
# Does NOT require the native extension: Namespace is pure Ruby.
#
# Cross-references:
#   - SPEC.md B-07 — Namespace name validation
#   - SPEC.md B-08 — Member name validation (non-symbol/non-string input)
#   - SPEC.md B-10 — define idempotence (duplicate Namespace)
#   - SPEC.md B-11 — duplicate bind raises, existing binding preserved

require "minitest/autorun"

$LOAD_PATH.unshift File.expand_path("../lib", __dir__)
require "kobako/registry"

module Kobako
  class RPCNamespaceEdgeCasesTest < Minitest::Test
    Klass = Kobako::RPC::Namespace

    # --- bind: non-symbol/non-string member name ---

    # SPEC B-08 Notes: bind validates the member name against the constant
    # pattern after coercing it to a String via #to_s.  Passing an Integer
    # hits the coercion path ("42".to_s → "42") then fails the pattern check
    # because "42" starts with a digit, not an uppercase letter.
    def test_bind_with_integer_member_name_raises_argument_error
      namespace = Klass.new("Logger")
      err = assert_raises(ArgumentError) { namespace.bind(42, Object.new) }
      assert_match(/MemberName/, err.message)
    end

    # An Array as member name produces a String via #to_s that cannot match
    # the constant pattern — another non-symbol/non-string path.
    def test_bind_with_array_member_name_raises_argument_error
      namespace = Klass.new("Logger")
      assert_raises(ArgumentError) { namespace.bind([:bad], Object.new) }
    end

    # --- duplicate bind (B-11) ---

    # Already covered in test_service_registry.rb (via Sandbox#define + bind),
    # but that test requires the native ext.  This version exercises the same
    # guarantee at the Namespace level without any ext dependency.
    def test_duplicate_bind_at_namespace_level_raises_and_preserves_original
      namespace = Klass.new("Logger")
      namespace.bind(:Info, :first)

      err = assert_raises(ArgumentError) { namespace.bind(:Info, :second) }
      assert_match(/already bound/, err.message)
      assert_equal :first, namespace.fetch("Info"),
                   "original binding must survive duplicate-bind attempt"
    end

    # --- empty namespace: to_preamble round-trip ---

    # SPEC B-07 Notes: an empty Namespace (no Members) is legal and its
    # to_preamble form is [name, []].  Verifies that guest_preamble does not
    # blow up on a Registry that contains only empty Namespaces.
    def test_empty_namespace_to_preamble_returns_empty_members_list
      namespace = Klass.new("Empty")
      assert_equal ["Empty", []], namespace.to_preamble
    end

    def test_registry_with_only_empty_namespace_produces_valid_preamble
      require "msgpack"
      registry = Kobako::Registry.new
      registry.define(:Empty)

      bytes = registry.guest_preamble
      decoded = MessagePack.unpack(bytes)
      assert_equal [["Empty", []]], decoded
    end

    # --- non-symbol string name accepted by bind ---

    # SPEC B-08: both Symbol and String forms of a constant-pattern name
    # must be accepted and normalized to the same String key internally.
    def test_bind_with_string_member_name_normalizes_to_string_key
      namespace = Klass.new("Logger")
      namespace.bind("Info", :v)
      assert_equal :v, namespace.fetch("Info")
      assert_equal :v, namespace["Info"]
    end

    # --- Namespace#[] returns nil for missing member ---

    def test_bracket_returns_nil_for_unknown_member
      namespace = Klass.new("Logger")
      namespace.bind(:Info, :val)
      assert_nil namespace["Missing"]
    end

    # --- Namespace#fetch raises KeyError for missing member ---

    def test_fetch_raises_key_error_for_unknown_member
      namespace = Klass.new("Logger")
      err = assert_raises(KeyError) { namespace.fetch("Unknown") }
      assert_match(/Unknown/, err.message)
    end
  end
end
