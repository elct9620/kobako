# frozen_string_literal: true

require "test_helper"
require "pp"

# A library mixing a method written in Ruby into every object hands the guest
# what the object never defined: ActiveSupport's +try+ dispatches by name,
# its +to_json+ dumps every instance variable, and +pp+ lists them. Each
# Service below permits every name, so only the floor stands in the way.
class TestDispatchAmbientMixins < Minitest::Test
  # The shape of ActiveSupport's +try+, included into Object.
  module DispatchByName
    def kobako_try(name, *) = public_send(name, *)
  end
  Object.include(DispatchByName)

  # The shape of ActiveSupport's +to_json+, prepended to Object.
  module DumpState
    def kobako_dump = instance_variables.to_h { |ivar| [ivar, instance_variable_get(ivar)] }
  end
  Object.prepend(DumpState)

  # A mixin of Module, reached on a bound class.
  module ListSubclasses
    def kobako_subclasses = subclasses.map(&:name)
  end
  Module.include(ListSubclasses)

  class Wide
    def initialize = @secret = "TOPSECRET"
    def own = "ok"

    private

    def respond_to_guest?(_name) = true
  end

  class WideClass
    def self.respond_to_guest?(_name) = true
    private_class_method :respond_to_guest?
  end

  ROWS = {
    "kobako_try" => %i[instance_variable_get @secret],
    "kobako_dump" => [],
    "pretty_print_instance_variables" => []
  }.freeze

  def setup
    @handles = Kobako::Catalog::Handles.new
    @services = Kobako::Catalog::Services.new
    @services.bind("Cfg::Wide", Wide.new)
    @services.bind("Cfg::Klass", WideClass)
  end

  def dispatch(target, method, args = [])
    call = DispatcherHelpers.call_for(target, method, args)
    DispatcherHelpers.reify(Kobako::Transport::Dispatcher.dispatch(call, @services, @handles,
                                                                   Kobako::Transport::BlockFailure.new,
                                                                   ->(_bytes) { raise "no block" }))
  end

  # @behavior T-298
  def test_a_permissive_predicate_reaches_no_method_mixed_into_every_object
    ROWS.each do |name, args|
      resp = dispatch("Cfg::Wide", name, args)
      assert_equal [false, "undefined"], [resp.ok?, (resp.payload.type unless resp.ok?)],
                   "#{name}, mixed into Object, through guest dispatch with a predicate permitting every " \
                   "name must be refused as undefined"
    end
    assert_equal [true, "ok"], dispatch("Cfg::Wide", "own").then { |resp| [resp.ok?, resp.payload] },
                 "the object's own method through guest dispatch must stay reachable beside the refused mixins"
  end

  # @behavior T-298
  def test_a_permissive_predicate_reaches_no_method_mixed_into_module
    resp = dispatch("Cfg::Klass", "kobako_subclasses")
    assert_equal [false, "undefined"], [resp.ok?, (resp.payload.type unless resp.ok?)],
                 "kobako_subclasses, mixed into Module, through guest dispatch on a bound class " \
                 "must be refused as undefined"
  end
end
