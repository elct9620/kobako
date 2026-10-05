# frozen_string_literal: true

require "test_helper"

# Unit tests for the singleton methods a bound object carries. Losing the
# class-level surface is what a bound class or module pays; an ordinary
# object's own singleton methods are authored on it and stay reachable.
class TestDispatchSingletons < Minitest::Test
  include DispatcherHelpers

  def setup
    super
    greeter = Object.new
    def greeter.greet = "hi"
    @registry.bind("Cfg::Greeter", greeter)
    @registry.seal!
  end

  # @behavior T-227
  def test_an_ordinary_object_keeps_its_singleton_methods_reachable
    resp = reify(dispatch(build_call("Cfg::Greeter", "greet")))

    assert_equal [true, "hi"], [resp.ok?, resp.payload],
                 "a singleton method on an ordinary bound object through guest dispatch must answer"
  end
end
