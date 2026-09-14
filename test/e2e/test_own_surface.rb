# frozen_string_literal: true

require "test_helper"
require "delegate"
require "stringio"

# E2E (Layer 4) — the default Exposure, driven through the real mruby guest
# (`data/kobako.wasm`). An object carrying no narrowing predicate exposes
# what its own class and the object itself define in source; everything it
# acquired elsewhere — a superclass, a mixed-in module, the platform, a
# forwarder's target, a dynamic handler — stays out of the guest's reach.
class TestE2EOwnSurface < Minitest::Test
  include E2eGuestHelper

  class Base
    def helper = "inherited"
  end

  class Store < Base
    def get = "own"
  end

  module Shared
    def shared = "mixed in"
  end

  class Mixed
    include Shared

    def own = "own"
  end

  Amount = Data.define(:cents)
  Point = Struct.new(:x)

  # Answers every name dynamically, and says so, without a predicate.
  class Dynamic
    def respond_to_missing?(_name, _include_private = false) = true
    def method_missing(name, *) = "dynamic:#{name}"
  end

  # A permissive public predicate in front of which a forwarder stands.
  class Permissive
    def read = "secret"
    def respond_to_guest?(_name) = true
  end

  def sandbox_with(path, object)
    Kobako::Sandbox.new.tap { |sandbox| sandbox.bind(path, object) }
  end

  # @behavior T-208
  def test_a_method_inherited_from_a_superclass_is_refused
    sandbox = sandbox_with("App::Store", Store.new)

    assert_equal "own", sandbox.eval("App::Store.get").value
    assert_raises(Kobako::NoServiceError, "an inherited method through a bound object must be refused") do
      sandbox.eval("App::Store.helper")
    end
  end

  # @behavior T-209
  def test_a_method_mixed_in_from_a_module_is_refused
    sandbox = sandbox_with("App::Mixed", Mixed.new)

    assert_raises(Kobako::NoServiceError, "a mixed-in method through a bound object must be refused") do
      sandbox.eval("App::Mixed.shared")
    end
  end

  # @behavior T-210
  def test_a_core_object_reached_through_a_reference_exposes_nothing
    sandbox = Kobako::Sandbox.new
    sandbox.preload(code: "Echo = ->(body) { body.read }", name: :Echo)

    assert_raises(Kobako::NoServiceError,
                  "a built-in method through a #run argument's Handle must be refused") do
      sandbox.run(:Echo, StringIO.new("hello"))
    end
  end

  # @behavior T-211
  def test_a_record_exposes_the_reader_of_each_member
    assert_equal 125, sandbox_with("App::Amount", Amount.new(cents: 125)).eval("App::Amount.cents").value,
                 "a member reader through a bound record must answer"
  end

  # @behavior T-212
  def test_a_record_exposes_no_built_in_writer
    sandbox = sandbox_with("App::Point", Point.new(1))

    assert_raises(Kobako::NoServiceError, "a built-in member writer through a bound record must be refused") do
      sandbox.eval("App::Point.x = 2")
    end
  end

  # @behavior T-213
  def test_a_method_defined_after_the_binding_is_not_reachable
    store = Store.new
    sandbox = sandbox_with("App::Store", store)
    def store.late = "late"

    assert_raises(Kobako::NoServiceError, "a method defined after bind through that binding must be refused") do
      sandbox.eval("App::Store.late")
    end
  end

  # @behavior T-214
  def test_a_forwarder_bound_directly_answers_none_of_its_forwarded_names
    sandbox = sandbox_with("App::Wrap", SimpleDelegator.new(Permissive.new))

    assert_raises(Kobako::NoServiceError, "a forwarded name through a bound forwarder must be refused") do
      sandbox.eval("App::Wrap.read")
    end
  end

  # @behavior T-215
  def test_a_dynamic_name_is_refused_without_a_predicate
    sandbox = sandbox_with("App::Dynamic", Dynamic.new)

    assert_raises(Kobako::NoServiceError,
                  "a dynamically answered name through an object without a predicate must be refused") do
      sandbox.eval("App::Dynamic.anything")
    end
  end
end
