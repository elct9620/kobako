# frozen_string_literal: true

require "test_helper"
require "delegate"

# The default Exposure is drawn around authorship: an object without a
# narrowing predicate of its own exposes what its own class and the object
# itself define in source. These cases pin the edges the real guest cannot
# conveniently reach — how a predicate is recognised, which definitions
# count as built in, and the per-table class cache — while
# test/e2e/test_own_surface.rb walks the same rule through a live Sandbox.
class TestExposure < Minitest::Test
  class Base
    def helper = :base
  end

  class Store < Base
    attr_reader :name

    def get = :got
    def to_s = "store"
  end

  # A predicate answering everything, reachable only through a public
  # definition — a forwarder in front of it must not borrow it.
  class Permissive
    def secret = :secret
    def respond_to_guest?(_name) = true
  end

  # Claims to respond to every name without defining any of them.
  class CatchAll
    def respond_to_missing?(_name, _include_private = false) = true
    def method_missing(name, *) = name
  end

  # Defines no respond_to? of its own, so any reflection on it has to be
  # bound from Kernel.
  class Bare < BasicObject
    def ping = :pong
  end

  def exposes?(object, name) = Kobako::Transport::Exposure.of(object).exposes?(name)

  def test_a_subclass_exposes_its_own_methods_and_accessors_but_not_its_parents
    store = Store.new

    %i[get name to_s].each do |own|
      assert exposes?(store, own), "#{own} defined in the object's own class through Exposure.of must be exposed"
    end
    refute exposes?(store, :helper), "a superclass method through Exposure.of must not be exposed"
  end

  def test_a_platform_method_written_in_ruby_is_built_in_too
    queue = Thread::Queue.new

    refute exposes?(queue, :pop),
           "a core method whose source is the platform's own through Exposure.of must not be exposed"
  end

  def test_only_public_singleton_methods_join_the_surface
    object = Object.new
    def object.ping = :pong
    object.singleton_class.class_eval { protected def hidden = :hidden }

    assert exposes?(object, :ping), "a public singleton method through Exposure.of must be exposed"
    refute exposes?(object, :hidden), "a protected singleton method through Exposure.of must not be exposed"
  end

  def test_a_private_singleton_predicate_narrows_the_object
    object = Store.new
    object.define_singleton_method(:respond_to_guest?) { |name| name == :name }
    object.singleton_class.class_eval { private :respond_to_guest? }

    assert exposes?(object, :name), "a name a singleton predicate permits through Exposure.of must be exposed"
    refute exposes?(object, :get), "a name a singleton predicate denies through Exposure.of must not be exposed"
  end

  def test_answering_every_name_is_not_a_predicate
    refute exposes?(CatchAll.new, :anything),
           "an object whose respond_to_missing? answers every name through Exposure.of must not expose dynamic names"
  end

  def test_a_forwarder_does_not_borrow_its_targets_predicate
    forwarder = SimpleDelegator.new(Permissive.new)

    refute exposes?(forwarder, :secret),
           "a forwarder in front of a permissive predicate through Exposure.of must expose nothing"
  end

  def test_a_forwarder_that_narrows_itself_is_asked
    guarded = Class.new(SimpleDelegator) { private def respond_to_guest?(name) = name == :secret }
                   .new(Permissive.new)

    assert exposes?(guarded, :secret),
           "a forwarder defining its own predicate through Exposure.of must be asked rather than emptied"
  end

  def test_a_basic_object_derives_without_its_own_reflection
    assert exposes?(Bare.new, :ping), "a BasicObject's own method through Exposure.of must be exposed"
  end

  def test_one_table_enumerates_a_class_once_and_still_adds_singletons
    surfaces = {}.compare_by_identity
    plain = Store.new
    tagged = Store.new
    def tagged.tag = :tag

    Kobako::Transport::Exposure.of(plain, surfaces)
    exposure = Kobako::Transport::Exposure.of(tagged, surfaces)

    assert_equal [Store], surfaces.keys, "two objects of one class through Exposure.of must share one cached surface"
    assert exposure.exposes?(:tag), "a singleton method through a cached class surface must still be exposed"
    refute Kobako::Transport::Exposure.of(Store.new, surfaces).exposes?(:tag),
           "a singleton method through a cached class surface must not leak to a sibling object"
  end
end
