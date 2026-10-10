# frozen_string_literal: true

require "test_helper"

# E2E — the guest-side Capability Handle / bound-constant proxy
# surface through real mruby: chaining a Service-returned Handle as the next
# dispatch target, respond_to? probing before dispatch, the capability-inert
# result of constructing a bound-constant proxy, and the blocked construction
# of the Handle proxy that keeps target derivation tied to host-issued
# identity.
class TestE2EHandleProxy < Minitest::Test
  include E2eGuestHelper

  # Stateful object handed to the chain tests — Factory::Make returns a
  # Greeter, the guest then routes greet() to it directly.
  class Greeter
    def initialize(name) = (@name = name)
    def greet = "hi,#{@name}"
  end

  # @behavior T-006
  def test_handle_chain_service_returns_handle_used_as_target
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("Factory::Make", ->(name) { Greeter.new(name) })

    result = sandbox.eval(<<~RUBY).value
      g = Factory::Make.call("Bob")
      g.greet
    RUBY

    assert_equal "hi,Bob", result,
                 "Handle target from first transport call routes second call to the stateful object"
  end

  # KV::Lookup exercises the bound-constant (class-level) registration; the
  # Greeter Handle exercises the Handle (instance-level) registration — one
  # assertion pins both paths.
  # @behavior T-108 T-190
  def test_respond_to_probe_succeeds_on_bound_constant_and_handle
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("KV::Lookup", ->(key) { "value:#{key}" })
    sandbox.bind("Factory::Make", ->(name) { Greeter.new(name) })

    result = sandbox.eval(<<~RUBY).value
      handle = Factory::Make.call("Bob")
      [KV::Lookup.respond_to?(:lookup_anything), handle.respond_to?(:greet)]
    RUBY

    assert_equal [true, true], result,
                 "respond_to? on a bound constant and on a Handle instance must both " \
                 "report true so guest-side capability probing succeeds before dispatch"
  end

  # Construction is not the capability gate; the host's path resolution is.
  # Pairing both readouts in one eval pins the contrast.
  # @behavior T-109
  def test_bound_proxy_construction_yields_a_capability_inert_instance
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("Models::User", Greeter.new("bound"))

    result = sandbox.eval(<<~RUBY).value
      [(Models::User.new.greet rescue "inert"), Models::User.greet]
    RUBY

    assert_equal %w[inert hi,bound], result,
                 "a constructed bound-constant proxy must forward nothing (inert), while the constant forwards"
  end

  # A Service that counts every call it receives, so a test can show the
  # host was never asked.
  class Counter
    attr_reader :calls

    def initialize = (@calls = 0)
    def greet = (@calls += 1)
  end

  # Construction is the guest's own business: neither entry asks the host,
  # and what it builds forwards nothing, so a method on it is missing.
  INERT_INSTANCES = <<~RUBY
    %w[new allocate].map do |entry|
      instance = Models::User.public_send(entry)
      begin
        instance.greet
        :forwarded
      rescue NoMethodError
        :no_method
      end
    end
  RUBY

  BUILT_INSTANCES = "%w[new allocate].map { |e| Models::User.public_send(e).is_a?(Models::User) }"

  # @behavior T-272
  def test_a_bound_proxy_instance_is_built_in_the_guest
    assert_equal [true, true], proxy_sandbox(Counter.new).eval(BUILT_INSTANCES).value,
                 "constructing a bound proxy by new or allocate through #eval must answer an instance"
  end

  # @behavior T-234
  def test_building_a_bound_proxy_instance_never_calls_the_bound_object
    counter = Counter.new
    proxy_sandbox(counter).eval(INERT_INSTANCES)

    assert_equal 0, counter.calls,
                 "constructing a bound proxy by new or allocate through #eval must never call the bound object"
  end

  # @behavior T-235
  def test_a_bound_proxy_instance_forwards_nothing
    assert_equal %i[no_method no_method], proxy_sandbox(Counter.new).eval(INERT_INSTANCES).value,
                 "a method on a bound proxy instance through #eval must raise NoMethodError in the guest"
  end

  # A proxy minted from a bare id would dispatch against an arbitrary
  # Catalog::Handles entry. The `.new(1)` case pins that an integer argument
  # does not change the outcome — the raise fires ahead of any arity check
  # (the reason the bridge registers `mrb_args_any()`); `.allocate` covers
  # mruby's other construction entry.
  # @behavior T-110 T-191
  def test_handle_proxy_is_not_constructible
    ["Kobako::Handle.new(1)", "Kobako::Handle.allocate"].each do |code|
      sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)

      err = assert_raises(Kobako::SandboxError) { sandbox.eval(code) }

      assert_equal "NoMethodError", err.klass,
                   "fabricating a Handle (#{code}) through the guest must raise " \
                   "NoMethodError, not mint a proxy from a bare id"
      assert_match(/Kobako::Handle/, err.message,
                   "the error must name Kobako::Handle so the author can locate it")
    end
  end

  private

  def proxy_sandbox(bound)
    Kobako::Sandbox.new(wasm_path: REAL_WASM).tap { |sandbox| sandbox.bind("Models::User", bound) }
  end
end
