# frozen_string_literal: true

require "test_helper"

# E2E (Layer 4) — the guest-side Capability Handle / bound-constant proxy
# surface through real mruby: chaining a Service-returned Handle as the next
# dispatch target (B-17), respond_to? probing before dispatch (B-36), the
# capability-inert result of constructing a bound-constant proxy (B-38), and
# the blocked construction of the Handle proxy that keeps target derivation
# tied to host-issued identity (B-39 / B-59).
class TestE2EHandleProxy < Minitest::Test
  include E2eGuestHelper

  # Stateful object handed to B-17 chain tests — Factory::Make returns a
  # Greeter, the guest then routes greet() to it directly.
  class Greeter
    def initialize(name) = (@name = name)
    def greet = "hi,#{@name}"
  end

  # SPEC.md B-17: Service A returns stateful object → guest uses Handle as
  # next transport target → chain works.
  # @behavior T-006
  def test_handle_chain_b17_service_returns_handle_used_as_target
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("Factory::Make", ->(name) { Greeter.new(name) })

    result = sandbox.eval(<<~RUBY).value
      g = Factory::Make.call("Bob")
      g.greet
    RUBY

    assert_equal "hi,Bob", result,
                 "B-17: Handle target from first transport call routes second call to the stateful object"
  end

  # SPEC.md B-36: a guest may probe a bound-Service constant or a Handle instance
  # with respond_to? before dispatching; both answer true because every
  # method forwards to the host. KV::Lookup exercises the bound-constant
  # (class-level) registration; the Greeter Handle exercises the Handle
  # (instance-level) registration — one assertion pins both paths.
  # @behavior T-108
  def test_b36_respond_to_probe_succeeds_on_bound_constant_and_handle
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("KV::Lookup", ->(key) { "value:#{key}" })
    sandbox.bind("Factory::Make", ->(name) { Greeter.new(name) })

    result = sandbox.eval(<<~RUBY).value
      handle = Factory::Make.call("Bob")
      [KV::Lookup.respond_to?(:lookup_anything), handle.respond_to?(:greet)]
    RUBY

    assert_equal [true, true], result,
                 "B-36: respond_to? on a bound constant and on a Handle instance must both " \
                 "report true so guest-side capability probing succeeds before dispatch"
  end

  # SPEC.md B-38: a bound-constant proxy forwards at class level, so its
  # forwarding seam never rides an instance. Constructing one is not blocked
  # — `Models::User.new` yields a plain instance — but that instance carries
  # no dispatch: a method on it raises NoMethodError in-guest instead of
  # forwarding, while the same method on the constant forwards to the bound
  # object. Construction is not the capability gate; the host's path
  # resolution is. Pairing both readouts in one eval pins the contrast.
  # @behavior T-109
  def test_b38_bound_proxy_construction_yields_a_capability_inert_instance
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("Models::User", Greeter.new("bound"))

    result = sandbox.eval(<<~RUBY).value
      [(Models::User.new.greet rescue "inert"), Models::User.greet]
    RUBY

    assert_equal %w[inert hi,bound], result,
                 "B-38: a constructed bound-constant proxy must forward nothing (inert), while the constant forwards"
  end

  # SPEC.md B-39: a Handle is a host-issued capability reference the wire
  # decoder constructs (B-14 / B-34); guest code has no path to fabricate
  # one. `Kobako::Handle.new(1)` / `.allocate` must raise NoMethodError
  # rather than mint a proxy from a bare id that would dispatch against an
  # arbitrary Catalog::Handles entry. Unrescued, it reaches the host as
  # SandboxError (E-04). The `.new(1)` case pins that an integer argument
  # does not change the outcome — the raise fires ahead of any arity check
  # (the reason the bridge registers `mrb_args_any()`); `.allocate` covers
  # mruby's other construction entry.
  # @behavior T-110
  def test_b39_handle_proxy_is_not_constructible
    ["Kobako::Handle.new(1)", "Kobako::Handle.allocate"].each do |code|
      sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)

      err = assert_raises(Kobako::SandboxError) { sandbox.eval(code) }

      assert_equal "NoMethodError", err.klass,
                   "B-39: fabricating a Handle (#{code}) through the guest must raise " \
                   "NoMethodError, not mint a proxy from a bare id"
      assert_match(/Kobako::Handle/, err.message,
                   "B-39: the error must name Kobako::Handle so the author can locate it")
    end
  end
end
