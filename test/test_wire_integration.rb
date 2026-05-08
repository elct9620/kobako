# frozen_string_literal: true

require "test_helper"

# Item #26 — Layer 2 (Wire integration through live Sandbox).
#
# SPEC.md §"Testing Style" Layer 2 (line 999): "Full Request / Response
# exchange through a live Sandbox, including the disconnected sentinel
# path and all envelope type variants." These tests drive the full
# guest→host RPC roundtrip through the real wasmtime + WASI pipeline,
# exercising `__kobako_rpc_call` (the host import wired in
# `ext/kobako/src/wasm.rs`) and the stdin two-frame source-delivery
# path. Unit-level coverage of `Registry#dispatch` lives in
# `test_rpc_dispatch.rb`; this file is the live-Sandbox elevation of
# that coverage.
#
# Each test bind a Service Member to the Sandbox, runs a fixture source
# that issues `__kobako_rpc_call` against the bound Service, and asserts
# what the host observes via `Sandbox#run` (return value or raised
# exception class). No test bypasses `Sandbox#run`.
#
# The test-guest fixture (`wasm/test-guest/src/lib.rs`) speaks four
# wire-integration source dialects:
#
#   * `rpc:G::M|method|arg`           — round-trip; err branch surfaces as
#                                       Result(Str("err:Nbytes"))
#   * `rpc-panic:G::M|method|arg`     — round-trip; err branch surfaces as
#                                       Panic(origin=service)
#   * `rpc-chain:G::F|fmth|arg|tmth`  — Handle target chaining (B-17)
#   * `rpc-kwargs:G::M|m|k1=v1,k2=v2` — kwargs-bearing Request
#   * `rpc-dc-chain:G::S|smth|tmth`   — disconnected-sentinel path (E-14)
class TestWireIntegration < Minitest::Test
  FIXTURE_PATH = File.expand_path("fixtures/test-guest.wasm", __dir__)

  def setup
    skip "native ext not compiled (run `bundle exec rake compile`)" unless defined?(Kobako::Wasm::Engine)
    skip "test-guest fixture missing (run `bundle exec rake fixtures:test_guest`)" \
      unless File.exist?(FIXTURE_PATH)

    @sandbox = Kobako::Sandbox.new(wasm_path: FIXTURE_PATH)
  end

  # -------- (1) + (5) Result envelope (success path) — string target ----

  # SPEC §B-12 — Request target as msgpack str routes through the
  # Registry's String branch. The full chain: guest builds Request
  # envelope, host imports `__kobako_rpc_call`, Registry#dispatch
  # resolves `"Group::Member"` to the bound Service Member, the Service
  # method's wire-representable return value flows back through the
  # Response envelope, and the fixture surfaces it as the run outcome.
  def test_string_target_returns_string_value_through_live_sandbox
    @sandbox.define(:Logger).bind(:Echo, lambda(&:upcase))

    assert_equal "HELLO", @sandbox.run("rpc:Logger::Echo|call|hello")
  end

  def test_string_target_returns_integer_value_through_live_sandbox
    @sandbox.define(:Math).bind(:Length, lambda(&:length))

    # The Service method returns an Integer; the wire codec round-trips
    # it as Value::Int and the fixture surfaces it as the outcome.
    assert_equal 5, @sandbox.run("rpc:Math::Length|call|hello")
  end

  def test_string_target_returns_nil_value_through_live_sandbox
    @sandbox.define(:Side).bind(:Effect, ->(_arg) {})

    assert_nil @sandbox.run("rpc:Side::Effect|call|x")
  end

  # -------- (2) Panic origin=service — Service raises StandardError -----

  # SPEC §"Error Scenarios" E-11: a bound Service method raising a Ruby
  # `StandardError` is reified as Response.err(type="runtime"). The
  # fixture's `rpc-panic:` branch then emits Panic(origin=service) so
  # the host's `Sandbox#run` raises `Kobako::ServiceError`.
  def test_service_method_raise_surfaces_as_service_error
    @sandbox.define(:Boom).bind(:Bang, ->(_arg) { raise "host error" })

    err = assert_raises(Kobako::ServiceError) do
      @sandbox.run("rpc-panic:Boom::Bang|call|x")
    end

    refute_kind_of Kobako::SandboxError, err,
                   "ServiceError must not be confused with SandboxError"
    refute_kind_of Kobako::TrapError, err,
                   "ServiceError must not be confused with TrapError"
    assert_equal "service", err.origin
    assert_match(/host error/, err.message)
  end

  # -------- (3) Panic origin=sandbox — guest-side mruby exception ------

  # SPEC §"Error Scenarios" E-04: a guest-side error reaching the top
  # level of `__kobako_run` is emitted as Panic(origin=sandbox). The
  # fixture's `panic` keyword simulates this by emitting a sandbox-origin
  # panic envelope directly. Through the live Sandbox, this surfaces as
  # `Kobako::SandboxError`.
  def test_guest_side_panic_surfaces_as_sandbox_error_through_live_sandbox
    err = assert_raises(Kobako::SandboxError) { @sandbox.run("panic") }

    refute_kind_of Kobako::ServiceError, err,
                   "SandboxError(origin=sandbox) must not be confused with ServiceError"
    refute_kind_of Kobako::TrapError, err,
                   "SandboxError must not be confused with TrapError"
    assert_equal "sandbox", err.origin
    assert_equal "RuntimeError", err.klass
    assert_equal "boom", err.message
  end

  # -------- (4) Disconnected sentinel — Handle resolves to :disconnected

  # SPEC §"Error Scenarios" E-14: a Request whose target Handle resolves
  # to the `:disconnected` sentinel in the HandleTable produces
  # Response.err(type="disconnected"). The fixture's `rpc-dc-chain:`
  # branch reifies that into Panic(origin=service,
  # class="Kobako::ServiceError::Disconnected") so the host's
  # `Sandbox#run` raises the disconnected subclass specifically.
  #
  # The setup Service Member must, in a single dispatch:
  #   1. allocate a host object in the HandleTable,
  #   2. mark that id `:disconnected` (B-19 ABA protection), and
  #   3. return the integer id so the fixture can re-target it.
  def test_handle_target_disconnected_surfaces_as_service_error_disconnected
    sandbox = @sandbox
    @sandbox.define(:Dc).bind(:Setup, lambda do
      id = sandbox.handle_table.alloc(Object.new)
      sandbox.handle_table.mark_disconnected(id)
      id
    end)

    err = assert_raises(Kobako::ServiceError::Disconnected) do
      @sandbox.run("rpc-dc-chain:Dc::Setup|call|noop")
    end

    assert_kind_of Kobako::ServiceError, err,
                   "Disconnected must inherit from ServiceError (SPEC §Error Class Hierarchy)"
    assert_equal "service", err.origin
    assert_equal "Kobako::ServiceError::Disconnected", err.klass
    assert_match(/disconnected/, err.message)
  end

  # -------- (6) Handle-target dispatch — B-17 chaining -----------------

  # SPEC §B-17: a Wire::Handle target arriving over the wire is resolved
  # against the HandleTable and dispatched directly. End-to-end chain:
  #   * RPC #1 — `Factory::Make.call("Alice")` returns a stateful Ruby
  #     object; B-14 wrap_return allocates Handle id (1) and the guest
  #     receives Value::Handle(1) in Response.ok.
  #   * RPC #2 — fixture issues a Request with Target::Handle(1) and
  #     method "greet"; resolve_target's Handle branch resolves id 1 to
  #     the live Greeter; the second call's value flows back.
  def test_handle_target_chaining_dispatches_to_handle_bound_object
    greeter_class = Class.new do
      def initialize(name) = (@name = name)
      def greet = "hi,#{@name}"
    end
    @sandbox.define(:Factory).bind(:Make, ->(name) { greeter_class.new(name) })

    assert_equal "hi,Alice", @sandbox.run("rpc-chain:Factory::Make|call|Alice|greet")
  end

  def test_handle_target_chaining_handle_id_is_resolved_through_handle_table
    # Defensive cross-check: the Handle id appears in the Sandbox's
    # HandleTable after the first RPC. The chain proves the same id is
    # resolved (not a stale lookup) by yielding the live object's value
    # rather than an undefined-target error.
    counter = Class.new do
      def initialize = (@n = 0)
      def tick = (@n += 1)
    end
    @sandbox.define(:F).bind(:Mk, ->(_arg) { counter.new })

    # tick increments and returns 1 — proves the second call landed on
    # the same Counter instance the first call produced.
    assert_equal 1, @sandbox.run("rpc-chain:F::Mk|call|seed|tick")
  end

  # -------- (7) kwargs symbolize at boundary — E-15 --------------------

  # SPEC §E-15 + §B-12 Notes: "Bound Ruby objects receive keyword
  # arguments as Ruby symbols, matching standard Ruby keyword argument
  # conventions." The wire kwargs map carries str keys; the Registry
  # symbolizes them at the boundary before public_send. End-to-end:
  # guest sends string keys, the Service method's signature is `(name:,
  # region:)`, the bound method receives Symbol keys.
  def test_kwargs_string_keys_become_symbols_at_dispatch_boundary
    klass = Class.new do
      define_method(:lookup) { |name:, region:| "#{region}/#{name}" }
    end
    @sandbox.define(:Geo).bind(:Lookup, klass.new)

    result = @sandbox.run("rpc-kwargs:Geo::Lookup|lookup|name=alice,region=us")

    assert_equal "us/alice", result
  end

  def test_kwargs_keyrest_method_receives_all_symbolized_keys
    klass = Class.new do
      attr_reader :captured

      def capture(**opts) = @captured = opts
    end
    obj = klass.new
    @sandbox.define(:K).bind(:Cap, obj)

    @sandbox.run("rpc-kwargs:K::Cap|capture|a=1,b=2")

    # The bound method received Symbol keys (not Strings) — the wire
    # carried str keys; the dispatcher symbolised them at the boundary.
    assert_equal({ a: "1", b: "2" }, obj.captured)
  end
end
