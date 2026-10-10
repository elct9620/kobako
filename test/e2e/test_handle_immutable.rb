# frozen_string_literal: true

require "test_helper"

# E2E — in-guest Handle immutability through real mruby. A
# decoder-minted Kobako::Handle is frozen, so the guest cannot re-point its
# id ivar (reflective mutation raises FrozenError) and a dup stays frozen,
# closing the forge / guess surface. A frozen Handle still dispatches,
# because the seam only reads the id.
class TestE2EHandleImmutable < Minitest::Test
  include E2eGuestHelper

  class Greeter
    def initialize(name) = (@name = name)
    def greet = "hi,#{@name}"
  end

  # Probe a held Handle: re-point its id ivar (capturing the raised exception
  # class), then collect greet / frozen? / dup.frozen? into one Array for the
  # outcome path. A frozen Handle raises FrozenError on the write yet greets.
  IMMUTABILITY_SCRIPT = <<~RUBY
    g = Factory::Make.call("Bob")
    repoint = begin
      g.instance_variable_set(:@__kobako_id__, 999)
      "mutated"
    rescue => e
      e.class.to_s
    end
    [g.greet, repoint, g.frozen?, g.dup.frozen?]
  RUBY

  # @behavior T-111 T-112 T-199 T-200
  def test_held_handle_is_frozen_and_still_dispatches
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("Factory::Make", ->(name) { Greeter.new(name) })

    result = sandbox.eval(IMMUTABILITY_SCRIPT).value

    assert_equal ["hi,Bob", "FrozenError", true, true], result,
                 "re-pointing a held Handle's id must raise FrozenError (immutable), " \
                 "while dup stays frozen and the Handle still dispatches"
  end

  # The other reflective writer, the clone entry, and a copy that keeps its
  # identity: each would be a way round the freeze if it were missing.
  COPY_AND_REWRITE_SCRIPT = <<~RUBY
    g = Factory::Make.call("Bob")
    rewrite = begin
      g.instance_eval { @__kobako_id__ = 999 }
      "mutated"
    rescue => e
      e.class.to_s
    end
    copy = g.dup
    same_id = copy.instance_variable_get(:@__kobako_id__) == g.instance_variable_get(:@__kobako_id__)
    [rewrite, g.clone.frozen?, same_id, copy.greet]
  RUBY

  # @behavior T-240
  def test_a_held_reference_resists_rewriting
    assert_equal "FrozenError", copy_and_rewrite[0],
                 "rewriting a held reference's identifier by instance_eval through #eval must raise FrozenError"
  end

  # @behavior T-241
  def test_a_clone_of_a_held_reference_is_frozen
    assert copy_and_rewrite[1], "a clone of a held reference through #eval must be frozen"
  end

  # @behavior T-242
  def test_a_copy_of_a_held_reference_keeps_its_identifier
    assert copy_and_rewrite[2], "a copy of a held reference through #eval must keep its identifier"
  end

  # @behavior T-273
  def test_a_copy_of_a_held_reference_dispatches_to_the_same_host_object
    assert_equal "hi,Bob", copy_and_rewrite[3],
                 "a copy of a held reference through #eval must dispatch to the same host object"
  end

  # dup and clone pass exactly one original, so reaching the hook with another
  # count takes a send. Holding it to that count keeps the hook from reading an
  # argument list it was never given.
  COPY_HOOK_ARITY_SCRIPT = <<~RUBY
    g = Factory::Make.call("Bob")
    begin
      g.send(:initialize_copy, g, g)
      "accepted"
    rescue => e
      e.class.to_s
    end
  RUBY

  # @behavior T-221
  def test_the_copy_hook_refuses_a_second_argument
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("Factory::Make", ->(name) { Greeter.new(name) })

    seen = sandbox.eval(COPY_HOOK_ARITY_SCRIPT).value

    assert_equal "ArgumentError", seen,
                 "a held Handle's copy hook reached with two arguments must be refused for " \
                 "its argument count"
  end

  private

  # Run COPY_AND_REWRITE_SCRIPT and answer its four observations in order.
  def copy_and_rewrite
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("Factory::Make", ->(name) { Greeter.new(name) })
    sandbox.eval(COPY_AND_REWRITE_SCRIPT).value
  end
end
