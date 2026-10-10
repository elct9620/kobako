# frozen_string_literal: true

require "test_helper"

# E2E — what identifies a capability reference to the guest→host
# paths, through real mruby. The Kobako::Proxy seam derives a Call target from
# the receiver's exact identity: an exact Kobako::Handle by its id, a class by
# its constant path. A receiver that mixed in the module without being either
# has no target and is refused in-guest, emitting no wire Call; an object
# merely bearing the reference's name carries no reference across as a value
# either, and a reference the guest receives carries the id the host issued
# whatever the guest redefined. The positive paths (bound constant, Handle)
# are pinned elsewhere.
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

  # Neither is the exact reference type nor a class the host bound: a
  # subclass inherits the closed construction entries, so no instance of it
  # ever exists, and a module answers no dispatch target. The Service
  # records whether it was asked.
  FORWARDING_IMPOSTORS = {
    "a subclass of the reference type" => <<~RUBY,
      class SubHandle < Kobako::Handle; end
      SubHandle.allocate.lookup(:x)
    RUBY
    "a guest module mixing in the forwarding seam" => <<~RUBY
      module Rogue; extend Kobako::Proxy; end
      Rogue.lookup(:x)
    RUBY
  }.freeze

  # @behavior T-243
  def test_an_impostor_of_the_reference_type_is_refused_in_the_guest
    FORWARDING_IMPOSTORS.each do |shape, script|
      err, = run_impostor(script)

      assert_equal "NoMethodError", err.klass, "#{shape} through #eval must be refused in the guest"
    end
  end

  # @behavior T-274
  def test_an_impostor_of_the_reference_type_never_reaches_the_service
    FORWARDING_IMPOSTORS.each do |shape, script|
      _, asked = run_impostor(script)

      assert_empty asked, "#{shape} through #eval must never reach the Service"
    end
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
    err = assert_raises(Kobako::SandboxError) { look_alike_sandbox.eval(LOOK_ALIKE_REFERENCE) }

    assert_equal "TypeError", err.klass,
                 "an object bearing a capability reference's name passed as a dispatch " \
                 "argument must be refused as the script's own type error"
  end

  # @behavior T-269
  def test_look_alike_reference_never_reaches_the_object_its_id_names
    assert_raises(Kobako::SandboxError) { look_alike_sandbox.eval(LOOK_ALIKE_REFERENCE) }

    refute @sink_reached,
           "an object bearing a capability reference's name must not reach the Service the " \
           "id it carries names"
  end

  Labeled = Struct.new(:label)

  # The guest reopens the reference type and pins every initialized reference
  # to the first one's identifier, so a reference built through that hook
  # would answer as "a". The second reference must still reach "b".
  REDEFINED_INITIALIZE = <<~RUBY
    $first_id = Factory::Make.call("a").instance_variable_get(:@__kobako_id__)
    class Kobako::Handle
      def initialize(_id)
        @__kobako_id__ = $first_id
      end
    end
    Factory::Make.call("b").label
  RUBY

  # @behavior T-265
  def test_a_received_reference_ignores_a_redefined_initializer
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("Factory::Make", ->(label) { Labeled.new(label) })

    assert_equal "b", sandbox.eval(REDEFINED_INITIALIZE).value,
                 "a reference received after the guest redefined Kobako::Handle#initialize " \
                 "through #eval must reach the object the host issued it for"
  end

  private

  # Run an impostor +script+ against a recording Service, then answer the
  # guest's failure and the keys the Service was asked for.
  def run_impostor(script)
    asked = []
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("KV::Lookup", ->(key) { asked << key })
    [assert_raises(Kobako::SandboxError) { sandbox.eval(script) }, asked]
  end

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
