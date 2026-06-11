# frozen_string_literal: true

require "test_helper"

# SPEC.md F-01 / F-08: Kobako::Sandbox.new + stdout/stderr capture with limits.
#
# Sandbox.new constructs the wasmtime pipeline (Engine / Module / Store /
# Instance) against the test fixture wasm, owns a per-instance Catalog::Handles,
# and holds the per-channel byte caches that back `#stdout` / `#stderr` /
# `#stdout_truncated?` / `#stderr_truncated?` (SPEC.md B-04). The per-
# channel cap itself is enforced inside the ext-owned WASI pipe.
class TestSandbox < Minitest::Test
  FIXTURE_PATH = File.expand_path("../fixtures/minimal_abi_ok.wat", __dir__)
  ABSENT_ABI_FIXTURE_PATH = File.expand_path("../fixtures/minimal.wasm", __dir__)
  MISMATCH_ABI_FIXTURE_PATH = File.expand_path("../fixtures/minimal_abi_mismatch.wat", __dir__)

  def setup
    skip "native ext not compiled (run `bundle exec rake compile`)" unless defined?(Kobako::Runtime)
    skip "minimal_abi_ok.wat fixture missing" unless File.exist?(FIXTURE_PATH)
  end

  def test_default_construction_exposes_wasm_path
    sandbox = Kobako::Sandbox.new(wasm_path: FIXTURE_PATH)

    assert_equal FIXTURE_PATH, sandbox.wasm_path
  end

  def test_default_construction_exposes_default_output_limits
    sandbox = Kobako::Sandbox.new(wasm_path: FIXTURE_PATH)

    assert_equal Kobako::SandboxOptions::DEFAULT_OUTPUT_LIMIT, sandbox.stdout_limit
    assert_equal Kobako::SandboxOptions::DEFAULT_OUTPUT_LIMIT, sandbox.stderr_limit
    assert_equal 1 << 20, sandbox.stdout_limit
  end

  # SPEC.md B-05: reading the capture channels before any +#run+ returns
  # an empty UTF-8 String; the truncation predicates default to +false+.
  def test_pre_run_capture_state_matches_b05
    sandbox = Kobako::Sandbox.new(wasm_path: FIXTURE_PATH)

    assert_equal "", sandbox.stdout
    assert_equal "", sandbox.stderr
    assert_equal Encoding::UTF_8, sandbox.stdout.encoding
    assert_equal Encoding::UTF_8, sandbox.stderr.encoding
    refute sandbox.stdout_truncated?
    refute sandbox.stderr_truncated?
  end

  def test_custom_limits_reflected
    sandbox = Kobako::Sandbox.new(wasm_path: FIXTURE_PATH, stdout_limit: 100, stderr_limit: 200)

    assert_equal 100, sandbox.stdout_limit
    assert_equal 200, sandbox.stderr_limit
  end

  def test_missing_wasm_raises_module_not_built_error
    assert_raises(Kobako::ModuleNotBuiltError) do
      Kobako::Sandbox.new(wasm_path: "/nonexistent/kobako.wasm")
    end
  end

  def test_eval_against_minimal_fixture_raises_trap_error_when_export_missing
    # The minimal_abi_ok.wat fixture passes construction but stubs only
    # the entry points — `__kobako_take_outcome` is absent, so the eval
    # step raises Kobako::TrapError directly from the ext; `#eval` only
    # adds the verb prefix. The user-facing message attributes the
    # failure to the public verb (`Sandbox#eval`) rather than the
    # underlying ABI symbol. Real fixture-based E2E coverage lives in
    # test/e2e/.
    sandbox = Kobako::Sandbox.new(wasm_path: FIXTURE_PATH)
    err = assert_raises(Kobako::TrapError) { sandbox.eval("nil") }
    assert_match(/Sandbox#eval failed/, err.message)
  end

  # docs/behavior.md B-40 / E-42: construction probes the guest's
  # __kobako_abi_version export and accepts only the host's own ABI
  # version. minimal.wasm predates the export (absent branch);
  # minimal_abi_mismatch.wat reports 9999 (non-equal branch). Both are
  # deterministic artifact faults, so they surface at Sandbox.new as
  # Kobako::SetupError — never mid-invocation.
  def test_construction_rejects_guest_without_abi_version_export
    skip "minimal.wasm fixture missing" unless File.exist?(ABSENT_ABI_FIXTURE_PATH)

    err = assert_raises(Kobako::SetupError) do
      Kobako::Sandbox.new(wasm_path: ABSENT_ABI_FIXTURE_PATH)
    end
    assert_match(/does not export __kobako_abi_version/, err.message)
  end

  def test_construction_rejects_guest_with_mismatched_abi_version
    skip "minimal_abi_mismatch.wat fixture missing" unless File.exist?(MISMATCH_ABI_FIXTURE_PATH)

    err = assert_raises(Kobako::SetupError) do
      Kobako::Sandbox.new(wasm_path: MISMATCH_ABI_FIXTURE_PATH)
    end
    assert_match(/reports ABI version 9999/, err.message)
  end

  def test_eval_rejects_non_string_code
    sandbox = Kobako::Sandbox.new(wasm_path: FIXTURE_PATH)
    err = assert_raises(Kobako::SandboxError) { sandbox.eval(nil) }
    assert_match(/must be a String/, err.message)
  end

  # Sandbox#define returns a Namespace usable for the bind chain — the
  # Sandbox-tier proof that #define delegates to Catalog::Namespaces rather
  # than dropping the call on the floor. Catalog::Namespaces's own contract
  # is pinned in test/catalog/test_namespaces.rb.
  def test_define_returns_namespace_usable_for_binding
    sandbox = Kobako::Sandbox.new(wasm_path: FIXTURE_PATH)
    namespace = sandbox.define(:Foo)

    assert_instance_of Kobako::Namespace, namespace
    assert_same namespace, namespace.bind(:Bar, :member)
  end

  # SPEC.md B-01: `timeout` defaults to 60 s (Float), `memory_limit`
  # to 1 MiB. Both surface as readonly attributes for introspection
  # by Host Apps that need to log policy. Pin the literal SPEC values
  # so the test catches a drift in either direction.
  def test_default_caps_match_spec_b01
    sandbox = Kobako::Sandbox.new(wasm_path: FIXTURE_PATH)

    assert_equal 60.0, sandbox.timeout
    assert_equal 1 << 20, sandbox.memory_limit
  end

  def test_custom_caps_reflected
    sandbox = Kobako::Sandbox.new(wasm_path: FIXTURE_PATH, timeout: 1.5, memory_limit: 2 << 20)

    assert_in_delta 1.5, sandbox.timeout, 1e-9
    assert_equal 2 << 20, sandbox.memory_limit
  end

  def test_nil_caps_disable_enforcement
    sandbox = Kobako::Sandbox.new(wasm_path: FIXTURE_PATH, timeout: nil, memory_limit: nil)

    assert_nil sandbox.timeout
    assert_nil sandbox.memory_limit
  end

  def test_integer_timeout_is_coerced_to_float
    sandbox = Kobako::Sandbox.new(wasm_path: FIXTURE_PATH, timeout: 5)

    assert_kind_of Float, sandbox.timeout
    assert_equal 5.0, sandbox.timeout
  end

  def test_invalid_timeout_raises_argument_error
    assert_raises(ArgumentError) { Kobako::Sandbox.new(wasm_path: FIXTURE_PATH, timeout: 0) }
    assert_raises(ArgumentError) { Kobako::Sandbox.new(wasm_path: FIXTURE_PATH, timeout: -1.0) }
    assert_raises(ArgumentError) { Kobako::Sandbox.new(wasm_path: FIXTURE_PATH, timeout: "60") }
  end

  def test_invalid_memory_limit_raises_argument_error
    assert_raises(ArgumentError) { Kobako::Sandbox.new(wasm_path: FIXTURE_PATH, memory_limit: 0) }
    assert_raises(ArgumentError) { Kobako::Sandbox.new(wasm_path: FIXTURE_PATH, memory_limit: -1) }
    assert_raises(ArgumentError) { Kobako::Sandbox.new(wasm_path: FIXTURE_PATH, memory_limit: 1.5) }
  end
end
