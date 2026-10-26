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

  # Copies mixins under names of its own, as OpenStruct aliases each public
  # method to +name!+ when it loads, so each copy's owner is this class.
  class Copies
    alias kobako_try! kobako_try
    define_method(:kobako_dump!, DumpState.instance_method(:kobako_dump))

    def initialize = @secret = "TOPSECRET"
    def own = "ok"
  end

  class WideCopies < Copies
    private

    def respond_to_guest?(_name) = true
  end

  COPIES = {
    "kobako_try!" => %i[instance_variable_get @secret],
    "kobako_dump!" => []
  }.freeze

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
    @services.bind("Cfg::Copies", Copies.new)
    @services.bind("Cfg::WideCopies", WideCopies.new)
  end

  def refused_copies(target)
    COPIES.to_h do |name, args|
      resp = dispatch(target, name, args)
      [name, [resp.ok?, (resp.payload.type unless resp.ok?)]]
    end
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

  # A copy's owner is the class that made it and its source is Ruby, so
  # neither the owner floor nor the source check alone tells it from the
  # class's own method; a bound object and a Handle are both reached.
  # @behavior T-299
  def test_an_object_narrowing_nothing_exposes_no_copy_of_a_mixed_in_method
    refused = COPIES.keys.to_h { |name| [name, [false, "undefined"]] }
    assert_equal [refused, refused],
                 [refused_copies("Cfg::Copies"), refused_copies(@handles.alloc(Copies.new).id)],
                 "a copy of a method mixed into Object, through guest dispatch on a bound object or a Handle " \
                 "narrowing nothing, must be refused as undefined"
    assert_equal [true, "ok"], dispatch("Cfg::Copies", "own").then { |resp| [resp.ok?, resp.payload] },
                 "the class's own method through guest dispatch must stay reachable beside the refused copies"
  end

  # @behavior T-300
  def test_a_permissive_predicate_reaches_no_copy_of_a_mixed_in_method
    refused = COPIES.keys.to_h { |name| [name, [false, "undefined"]] }
    assert_equal refused, refused_copies("Cfg::WideCopies"),
                 "a copy of a method mixed into Object, through guest dispatch with a predicate permitting " \
                 "every name, must be refused as undefined"
  end
end
