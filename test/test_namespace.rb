# frozen_string_literal: true

# Unit tests for Kobako::Namespace — the per-Sandbox name→Service binding
# table. Pure Ruby; does NOT require the native extension. Registry-level
# behaviour (define / lookup / seal! / encode) lives in
# test/catalog/test_namespaces.rb.
#
# Cross-references:
#   - SPEC.md / docs/behavior.md B-08 — Member binding accepts class/instance/module
#   - SPEC.md / docs/behavior.md B-11 — Duplicate bind raises, existing binding preserved
#   - SPEC.md / docs/behavior.md E-17 — Malformed Member name

require "minitest/autorun"

$LOAD_PATH.unshift File.expand_path("../lib", __dir__)
require "kobako/namespace"

module Kobako
  class NamespaceTest < Minitest::Test
    # ---------- bind: non-symbol/non-string member name (E-17) ----------

    # SPEC B-08 Notes / E-17: bind coerces the member name to a String via
    # #to_s before pattern-matching against the constant form. Integer 42
    # becomes "42" and fails the leading-letter check.
    def test_bind_with_integer_member_name_raises_argument_error
      namespace = Namespace.new("Logger")
      err = assert_raises(ArgumentError) { namespace.bind(42, Object.new) }
      assert_match(/MemberName/, err.message)
    end

    # An Array coerces via #to_s to a String that cannot match the
    # constant pattern — another non-symbol/non-string path.
    def test_bind_with_array_member_name_raises_argument_error
      namespace = Namespace.new("Logger")
      assert_raises(ArgumentError) { namespace.bind([:bad], Object.new) }
    end

    # E-17: lowercase and dashed forms reach the pattern check after #to_s.
    def test_bind_with_malformed_string_member_name_raises_argument_error
      namespace = Namespace.new("Logger")
      assert_raises(ArgumentError) { namespace.bind(:lower, Object.new) }
      assert_raises(ArgumentError) { namespace.bind(:"Has-Dash", Object.new) }
    end

    # ---------- duplicate bind (B-11) ----------

    def test_duplicate_bind_raises_and_preserves_original
      namespace = Namespace.new("Logger")
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
      namespace = Namespace.new("Logger")
      namespace.bind("Info", :v)
      assert_equal :v, namespace.fetch("Info")
    end

    # ---------- fetch raises for missing member ----------

    def test_fetch_raises_key_error_for_unknown_member
      namespace = Namespace.new("Logger")
      err = assert_raises(KeyError) { namespace.fetch("Unknown") }
      assert_match(/Unknown/, err.message)
    end
  end
end
