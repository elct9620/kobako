# frozen_string_literal: true

require "test_helper"

# E2E (Layer 4) — host-object restoration of a Capability Handle returned
# across the boundary through real mruby: a Handle the guest
# received and then hands back — as the #eval result, nested in a container,
# in a Hash key, or as a yield-block result — is restored to the original
# host object. A Handle broken out of a guest block is the exception:
# it rides back to the guest untouched and still routes to that object.
class TestE2EHandleRestoration < Minitest::Test
  include E2eGuestHelper

  # Stateful host object bound behind Source::Get so restoration pins
  # identity, not just equality.
  class Greeter
    def initialize(name) = (@name = name)
    def greet = "hi,#{@name}"
  end

  # Source binds a fixed instance so the test can pin identity, not just
  # equality.
  # @behavior T-054
  def test_returned_handle_is_restored_to_the_original_host_object
    greeter = Greeter.new("Bob")
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("Source::Get", -> { greeter })

    result = sandbox.eval("Source::Get.call").value

    assert_same greeter, result,
                "a Capability Handle returned as the #eval result must arrive at the " \
                "Host App as the original host object, never a Kobako::Handle"
  end

  # @behavior T-055
  def test_returned_handle_is_restored_inside_nested_containers
    greeter = Greeter.new("Bob")
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("Source::Get", -> { greeter })

    result = sandbox.eval("g = Source::Get.call; { list: [g], pair: g }").value

    assert_same greeter, result[:list][0],
                "a Handle nested in an Array leaf must be restored to its host object"
    assert_same greeter, result[:pair],
                "a Handle in a Hash value must be restored to its host object"
  end

  # A Handle is wire-representable, so the guest may legitimately build a
  # Hash keyed by one; left unresolved, host code would receive a raw
  # Kobako::Handle where it expects the object.
  # @behavior T-056
  def test_returned_handle_is_restored_in_hash_key_position
    greeter = Greeter.new("Bob")
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("Source::Get", -> { greeter })

    result = sandbox.eval('g = Source::Get.call; { g => "label" }').value

    assert_same greeter, result.keys.first,
                "a Handle in a Hash key must be restored to its host object, symmetric " \
                "with the Array-element and Hash-value positions"
    assert_equal "label", result[greeter],
                 "the restored Hash key must still map to its original value"
  end

  # Sink::Run captures its block's return value so the test observes what
  # the yield site received.
  # @behavior T-057
  def test_returned_handle_is_restored_on_the_yield_block_result
    greeter = Greeter.new("Bob")
    captured = nil
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("Source::Get", -> { greeter })
    sandbox.bind("Sink::Run", ->(&blk) { captured = blk.call })

    sandbox.eval("Sink::Run.call { Source::Get.call }")

    assert_same greeter, captured,
                "a Handle returned from a guest block must reach the Service yield site " \
                "as the original host object"
  end

  # The break value returns to the guest bound-constant call, not to host
  # code, so there is nothing for the host to restore.
  # @behavior T-058
  def test_broken_handle_returns_to_guest_and_still_routes_to_host_object
    greeter = Greeter.new("Bob")
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("Source::Get", -> { greeter })
    sandbox.bind("Probe::Each", ->(items, &blk) { items.each(&blk) })

    result = sandbox.eval(
      "h = Source::Get.call; found = Probe::Each.call([1, 2, 3]) { |x| break h if x == 2 }; found.greet"
    ).value

    assert_equal "hi,Bob", result,
                 "a Handle broken out of a guest block returns to the guest and still " \
                 "routes a later call to the original host object"
  end
end
