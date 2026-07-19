# frozen_string_literal: true

require "test_helper"

# E2E (Layer 4) — Capability Handle behaviour through real mruby: chaining a
# Service-returned Handle as the next dispatch target (B-17), respond_to?
# probing (B-36), host-object restoration on every return path (B-37), the
# capability-inert result of constructing a bound-constant proxy (B-38), the
# blocked construction of the Handle proxy (B-39), and the exact-identity
# target derivation that refuses a foreign proxy holder (B-59).
class TestE2EHandles < Minitest::Test
  include E2eGuestHelper

  # Stateful object handed to B-17 chain tests — Factory::Make returns a
  # Greeter, the guest then routes greet() to it directly.
  class Greeter
    def initialize(name) = (@name = name)
    def greet = "hi,#{@name}"
  end

  # SPEC.md B-17: Service A returns stateful object → guest uses Handle as
  # next transport target → chain works.
  def test_handle_chain_b17_service_returns_handle_used_as_target
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("Factory::Make", ->(name) { Greeter.new(name) })

    result = sandbox.eval(<<~RUBY)
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
  def test_b36_respond_to_probe_succeeds_on_bound_constant_and_handle
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("KV::Lookup", ->(key) { "value:#{key}" })
    sandbox.bind("Factory::Make", ->(name) { Greeter.new(name) })

    result = sandbox.eval(<<~RUBY)
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
  def test_b38_bound_proxy_construction_yields_a_capability_inert_instance
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("Models::User", Greeter.new("bound"))

    result = sandbox.eval(<<~RUBY)
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

  # SPEC.md B-37: a Handle the guest received (here from Source::Get) and
  # then returns as the #eval result is restored on the host to the very
  # object Catalog::Handles holds — Source binds a fixed instance so the
  # test can pin identity, not just equality.
  def test_b37_returned_handle_is_restored_to_the_original_host_object
    greeter = Greeter.new("Bob")
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("Source::Get", -> { greeter })

    result = sandbox.eval("Source::Get.call")

    assert_same greeter, result,
                "B-37: a Capability Handle returned as the #eval result must arrive at the " \
                "Host App as the original host object, never a Kobako::Handle"
  end

  # SPEC.md B-37: the restoration walks nested Array / Hash, so a Handle in
  # any leaf position resolves to its host object while the surrounding
  # structure is preserved.
  def test_b37_returned_handle_is_restored_inside_nested_containers
    greeter = Greeter.new("Bob")
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("Source::Get", -> { greeter })

    result = sandbox.eval("g = Source::Get.call; { list: [g], pair: g }")

    assert_same greeter, result[:list][0],
                "B-37: a Handle nested in an Array leaf must be restored to its host object"
    assert_same greeter, result[:pair],
                "B-37: a Handle in a Hash value must be restored to its host object"
  end

  # SPEC.md B-37: restoration walks Hash keys as well as values. A Handle is
  # wire-representable, so the guest may legitimately build a Hash keyed by
  # one; the host must resolve that key to its object like any other leaf, or
  # host code would receive a raw Kobako::Handle where it expects the object.
  def test_b37_returned_handle_is_restored_in_hash_key_position
    greeter = Greeter.new("Bob")
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("Source::Get", -> { greeter })

    result = sandbox.eval('g = Source::Get.call; { g => "label" }')

    assert_same greeter, result.keys.first,
                "B-37: a Handle in a Hash key must be restored to its host object, symmetric " \
                "with the Array-element and Hash-value positions"
    assert_equal "label", result[greeter],
                 "B-37: the restored Hash key must still map to its original value"
  end

  # SPEC.md B-37 (yield path): a guest block that returns a Handle hands the
  # original host object back to the Service's yield expression, not a
  # Kobako::Handle token. Sink::Run captures its block's return value so the
  # test observes what the yield site received.
  def test_b37_returned_handle_is_restored_on_the_yield_block_result
    greeter = Greeter.new("Bob")
    captured = nil
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("Source::Get", -> { greeter })
    sandbox.bind("Sink::Run", ->(&blk) { captured = blk.call })

    sandbox.eval("Sink::Run.call { Source::Get.call }")

    assert_same greeter, captured,
                "B-37: a Handle returned from a guest block must reach the Service yield site " \
                "as the original host object"
  end

  # SPEC.md B-25 / B-37: a Handle broken out of a guest block is NOT restored
  # — the break value returns to the guest bound-constant call, not to host code — so
  # it rides back as a Handle the guest can still route through to the
  # original host object on a later call.
  def test_b37_broken_handle_returns_to_guest_and_still_routes_to_host_object
    greeter = Greeter.new("Bob")
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("Source::Get", -> { greeter })
    sandbox.bind("Probe::Each", ->(items, &blk) { items.each(&blk) })

    result = sandbox.eval(
      "h = Source::Get.call; found = Probe::Each.call([1, 2, 3]) { |x| break h if x == 2 }; found.greet"
    )

    assert_equal "hi,Bob", result,
                 "B-25/B-37: a Handle broken out of a guest block returns to the guest and still " \
                 "routes a later call to the original host object"
  end
end
