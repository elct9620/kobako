# frozen_string_literal: true

require "test_helper"

# E2E (Layer 4) — host-object restoration of a Capability Handle returned
# across the boundary through real mruby (SPEC.md B-37): a Handle the guest
# received and then hands back — as the #eval result, nested in a container,
# in a Hash key, or as a yield-block result — is restored to the original
# host object. A Handle broken out of a guest block (B-25) is the exception:
# it rides back to the guest untouched and still routes to that object.
class TestE2EHandleRestoration < Minitest::Test
  include E2eGuestHelper

  # Stateful host object bound behind Source::Get so restoration pins
  # identity, not just equality.
  class Greeter
    def initialize(name) = (@name = name)
    def greet = "hi,#{@name}"
  end

  # SPEC.md B-37: a Handle the guest received (here from Source::Get) and
  # then returns as the #eval result is restored on the host to the very
  # object Catalog::Handles holds — Source binds a fixed instance so the
  # test can pin identity, not just equality.
  def test_b37_returned_handle_is_restored_to_the_original_host_object
    greeter = Greeter.new("Bob")
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("Source::Get", -> { greeter })

    result = sandbox.eval("Source::Get.call").value

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

    result = sandbox.eval("g = Source::Get.call; { list: [g], pair: g }").value

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

    result = sandbox.eval('g = Source::Get.call; { g => "label" }').value

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
    ).value

    assert_equal "hi,Bob", result,
                 "B-25/B-37: a Handle broken out of a guest block returns to the guest and still " \
                 "routes a later call to the original host object"
  end
end
