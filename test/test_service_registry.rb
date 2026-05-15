# frozen_string_literal: true

require "test_helper"

# Kobako::Registry + Kobako::RPC::Namespace + bind/define API.
#
# This is an integration-flavored Minitest covering SPEC B-07..B-11 on the
# Sandbox surface. The native ext is required only because Sandbox itself
# constructs the wasmtime pipeline; tests skip when it is absent.
class TestServiceRegistry < Minitest::Test
  FIXTURE_PATH = File.expand_path("fixtures/minimal.wasm", __dir__)

  def setup
    skip "native ext not compiled (run `bundle exec rake compile`)" unless defined?(Kobako::Wasm::Instance)
    skip "minimal.wasm fixture missing" unless File.exist?(FIXTURE_PATH)

    @sandbox = Kobako::Sandbox.new(wasm_path: FIXTURE_PATH)
  end

  # B-07: define returns a Kobako::RPC::Namespace; bind happy path resolves
  # via the two-level path on the Sandbox-owned Registry.
  def test_b07_define_returns_namespace_and_bind_resolves_member
    logger = Object.new
    def logger.info(msg) = "logged:#{msg}"

    group = @sandbox.define(:Logger)
    assert_instance_of Kobako::RPC::Namespace, group

    chain_target = group.bind(:Info, logger)
    assert_same group, chain_target, "bind must return self for chaining (B-08)"

    assert_same logger, @sandbox.services.lookup("Logger::Info")
    assert @sandbox.services.bound?("Logger::Info")
  end

  # B-08: bind accepts class / instance / module uniformly.
  def test_b08_bind_accepts_class_instance_and_module
    klass, instance, mod = b08_class_instance_module_triple
    @sandbox.define(:Mixed).bind(:K, klass).bind(:I, instance).bind(:M, mod)

    assert_same klass,    @sandbox.services.lookup("Mixed::K")
    assert_same instance, @sandbox.services.lookup("Mixed::I")
    assert_same mod,      @sandbox.services.lookup("Mixed::M")
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

  # B-09: multiple groups coexist; each binding resolves independently.
  def test_b09_multiple_groups_resolve_independently
    @sandbox.define(:Auth).bind(:Token, "tk")
    @sandbox.define(:Logger).bind(:Info, "lg")

    assert_equal "tk", @sandbox.services.lookup("Auth::Token")
    assert_equal "lg", @sandbox.services.lookup("Logger::Info")
    assert_equal 2, @sandbox.services.size
  end

  # B-09: cross-group paths do not leak — a member of one group is not
  # reachable via another group's name.
  def test_b09_cross_group_paths_are_not_bound
    @sandbox.define(:Auth).bind(:Token, "tk")
    @sandbox.define(:Logger).bind(:Info, "lg")

    refute @sandbox.services.bound?("Auth::Info")
    refute @sandbox.services.bound?("Logger::Token")
  end

  # B-10: re-declaring the same group is idempotent — same object identity.
  def test_b10_define_is_idempotent_and_preserves_members
    first = @sandbox.define(:Auth)
    first.bind(:Token, :original)

    second = @sandbox.define(:Auth)
    assert_same first, second, "define must return the identical Group on repeat"

    assert_equal :original, @sandbox.services.lookup("Auth::Token")
    assert_equal 1, @sandbox.services.size
  end

  # B-11: duplicate bind raises; the existing binding is preserved.
  def test_b11_duplicate_bind_raises_and_preserves_existing
    group = @sandbox.define(:Logger)
    group.bind(:Info, :first)

    err = assert_raises(ArgumentError) { group.bind(:Info, :second) }
    assert_match(/already bound/, err.message)
    assert_equal :first, @sandbox.services.lookup("Logger::Info"),
                 "existing binding must not be overwritten on duplicate-bind"
  end

  # SPEC B-07 Notes / E-16: malformed group name raises ArgumentError.
  def test_define_with_invalid_group_name_raises
    assert_raises(ArgumentError) { @sandbox.define(:lower) }
    assert_raises(ArgumentError) { @sandbox.define(:"Has-Dash") }
    assert_raises(ArgumentError) { @sandbox.define("9Numeric") }
  end

  # SPEC B-08 Notes / E-17: malformed member name raises ArgumentError.
  def test_bind_with_invalid_member_name_raises
    group = @sandbox.define(:Logger)
    assert_raises(ArgumentError) { group.bind(:lower, Object.new) }
    assert_raises(ArgumentError) { group.bind(:"Has-Dash", Object.new) }
  end

  # Unknown member: lookup raises with a clear message; bound? is false.
  def test_lookup_unknown_member_raises_clear_error
    @sandbox.define(:Logger) # no members bound

    err = assert_raises(KeyError) { @sandbox.services.lookup("Logger::Missing") }
    assert_match(/Missing/, err.message)
    refute @sandbox.services.bound?("Logger::Missing")
  end

  def test_lookup_unknown_group_raises_clear_error
    err = assert_raises(KeyError) { @sandbox.services.lookup("Ghost::Member") }
    assert_match(/Ghost/, err.message)
    refute @sandbox.services.bound?("Ghost::Member")
  end

  # Per-Sandbox isolation: two Sandboxes have independent Registries.
  def test_b09_per_sandbox_isolation
    other = Kobako::Sandbox.new(wasm_path: FIXTURE_PATH)

    @sandbox.define(:Logger).bind(:Info, :a_logger)
    refute other.services.bound?("Logger::Info"),
           "binding on one Sandbox must not leak to another"

    other.define(:Logger).bind(:Info, :b_logger)
    assert_equal :a_logger, @sandbox.services.lookup("Logger::Info")
    assert_equal :b_logger, other.services.lookup("Logger::Info")
  end

  # B-07 Notes: define after #run raises ArgumentError.
  # The minimal.wasm fixture has no SPEC ABI exports, so #run trips on
  # `__kobako_alloc` and raises Kobako::TrapError — but seal! has
  # already fired by then, so the registry transitions to sealed and
  # the post-run #define enforcement still applies.
  def test_b07_define_after_run_raises
    @sandbox.define(:Early).bind(:Member, :before_run)

    assert_raises(Kobako::TrapError) { @sandbox.run("nil") }
    assert @sandbox.services.sealed?

    err = assert_raises(ArgumentError) { @sandbox.define(:Late) }
    assert_match(/after Sandbox#run/, err.message)

    # Pre-run bindings remain accessible.
    assert_equal :before_run, @sandbox.services.lookup("Early::Member")
  end

  # `Group#to_preamble` returns the structured Frame 1 shape.
  def test_to_preamble_shape_matches_spec
    @sandbox.define(:MyService).bind(:KV, :kv).bind(:Logger, :log)
    @sandbox.define(:Auth).bind(:Token, :tk)

    assert_equal(
      [["MyService", %w[KV Logger]], ["Auth", %w[Token]]],
      @sandbox.services.to_preamble
    )
  end

  # Sandbox#services replacement check — no longer the placeholder.
  def test_services_is_no_longer_placeholder
    assert_instance_of Kobako::Registry, @sandbox.services
    refute @sandbox.services.class.name.include?("Placeholder"),
           "ServicesPlaceholder must be gone after item #15"
  end

  # Group string-name form is also accepted (symbol-or-string is accepted;
  # the user-facing form is symbol per SPEC examples but to_s is documented).
  def test_define_accepts_string_name_form
    group = @sandbox.define("Logger")
    assert_equal "Logger", group.name
    group.bind("Info", :v)
    assert_equal :v, @sandbox.services.lookup("Logger::Info")
  end

  # Item #25 — `guest_preamble` returns msgpack-encoded bytes matching the
  # two-level preamble array structure ({SPEC.md B-02}[link:../../SPEC.md]).
  def test_guest_preamble_returns_msgpack_encoded_preamble
    require "msgpack"
    @sandbox.define(:MyService).bind(:KV, :kv).bind(:Logger, :log)
    @sandbox.define(:Auth).bind(:Token, :tk)

    bytes = @sandbox.services.guest_preamble
    assert_kind_of String, bytes
    assert_equal Encoding::ASCII_8BIT, bytes.encoding

    decoded = MessagePack.unpack(bytes)
    assert_equal [["MyService", %w[KV Logger]], ["Auth", %w[Token]]], decoded
  end

  # Item #25 — empty registry produces a valid `[]` msgpack array as Frame 1.
  def test_guest_preamble_empty_registry_is_valid_msgpack_array
    require "msgpack"
    bytes = @sandbox.services.guest_preamble
    decoded = MessagePack.unpack(bytes)
    assert_equal [], decoded
  end
end
