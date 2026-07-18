# frozen_string_literal: true

require "test_helper"

# E2E (Layer 4) — in-guest Handle immutability through real mruby. A
# decoder-minted Kobako::Handle is frozen, so the guest cannot re-point its
# id ivar (reflective mutation raises FrozenError) and a dup stays frozen,
# closing the forge / guess surface (B-60). A frozen Handle still dispatches,
# because the seam only reads the id.
class TestE2EHandleImmutable < Minitest::Test
  include E2eGuestHelper

  class Greeter
    def initialize(name) = (@name = name)
    def greet = "hi,#{@name}"
  end

  def test_b60_held_handle_is_frozen_and_still_dispatches
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("Factory::Make", ->(name) { Greeter.new(name) })

    result = sandbox.eval(<<~RUBY)
      g = Factory::Make.call("Bob")
      repoint = (g.instance_variable_set(:@__kobako_id__, 999); "mutated") rescue "frozen"
      [g.greet, repoint, g.frozen?, g.dup.frozen?]
    RUBY

    assert_equal ["hi,Bob", "frozen", true, true], result,
                 "B-60: a held Handle must be frozen (re-point raises, dup stays frozen) yet still dispatch"
  end
end
