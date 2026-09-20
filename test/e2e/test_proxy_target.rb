# frozen_string_literal: true

require "test_helper"

# E2E (Layer 4) — what identifies a capability reference to the guest→host
# paths, through real mruby. The Kobako::Proxy seam derives a Call target from
# the receiver's exact identity: an exact Kobako::Handle by its id, a class by
# its constant path. A receiver that mixed in the module without being either
# has no target and is refused in-guest, emitting no wire Call; an object
# merely bearing the reference's name carries no reference across as a value
# either. The positive paths (bound constant, Handle) are pinned elsewhere.
class TestE2EProxyTarget < Minitest::Test
  include E2eGuestHelper

  # A guest class that mixes in Kobako::Proxy is neither an exact
  # Kobako::Handle nor a class, so method_missing finds no target and refuses
  # before any wire Call; uncaught it surfaces as SandboxError.
  # @behavior T-113 T-192
  def test_foreign_proxy_holder_is_refused_in_guest
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("KV::Lookup", ->(key) { "value:#{key}" })

    err = assert_raises(Kobako::SandboxError) do
      sandbox.eval("class Rogue; include Kobako::Proxy; end; Rogue.new.lookup(:x)")
    end
    assert_equal "NoMethodError", err.klass,
                 "a guest object that mixed in Kobako::Proxy without being a Handle must be refused in-guest"
    assert_match(/not a Kobako dispatch target/, err.message,
                 "the in-guest refusal must name the missing-target reason")
  end

  # A class of the guest's own bound to Kobako::Handle answers that name to
  # every name-based reading, so only the class the bridge registered can say
  # what a reference is. The id it carries is one the host really issued, so
  # the refusal is the identity check rather than an unknown id.
  LOOK_ALIKE_REFERENCE = <<~RUBY
    ref = Factory::Make.call
    id = ref.instance_variable_get(:@__kobako_id__)
    Kobako.const_set(:Handle, Class.new)
    fake = Kobako::Handle.new
    fake.instance_variable_set(:@__kobako_id__, id)
    Sink::Receive.call(fake)
  RUBY

  # @behavior T-220
  def test_look_alike_reference_carries_nothing_across_as_a_value
    sandbox = look_alike_sandbox

    err = assert_raises(Kobako::SandboxError) { sandbox.eval(LOOK_ALIKE_REFERENCE) }

    assert_equal "TypeError", err.klass,
                 "an object bearing a capability reference's name passed as a dispatch " \
                 "argument must be refused as the script's own type error"
    refute @sink_reached,
           "an object bearing a capability reference's name must not reach the Service the " \
           "id it carries names"
  end

  private

  # A Sandbox whose first Service answers an object the wire cannot carry, so
  # the guest holds a real reference, and whose second records being reached.
  def look_alike_sandbox
    @sink_reached = false
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("Factory::Make", -> { Object.new })
    sandbox.bind("Sink::Receive", ->(_value) { @sink_reached = true })
    sandbox
  end
end
