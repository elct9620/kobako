# frozen_string_literal: true

require "test_helper"
require "delegate"

# A narrowing predicate decides among the methods the Host App wrote. Each
# Service below permits every name yet holds a method Ruby defines, reached
# by a path other than Kernel's own, so an owner list alone would miss it.
class TestDispatchAuthorship < Minitest::Test
  class Inner
    def hello = "inner"
  end

  class Forwarder < SimpleDelegator
    private

    def respond_to_guest?(_name) = true
  end

  class KernelCopy
    include Kernel.dup

    private

    def respond_to_guest?(_name) = true
  end

  class PrivateForwarder
    def respond_to_missing?(_name, _include_private = false) = true
    def method_missing(name, *) = Object.new.__send__(name, *)

    private

    def respond_to_guest?(_name) = true
  end

  class AliasedBuiltin
    alias report format
    public :report

    private

    def respond_to_guest?(_name) = true
  end

  class BuiltFromBuiltin
    define_method(:report, Kernel.instance_method(:inspect))

    private

    def respond_to_guest?(_name) = true
  end

  ROWS = {
    "Cfg::Forward" => "instance_variable_get",
    "Cfg::Copy" => "instance_variable_get",
    "Cfg::Private" => "format",
    "Cfg::Alias" => "report",
    "Cfg::Built" => "report"
  }.freeze

  def setup
    @handles = Kobako::Catalog::Handles.new
    @services = Kobako::Catalog::Services.new
    { Forward: Forwarder.new(Inner.new), Copy: KernelCopy.new, Private: PrivateForwarder.new,
      Alias: AliasedBuiltin.new, Built: BuiltFromBuiltin.new }
      .each { |name, service| @services.bind("Cfg::#{name}", service) }
  end

  def dispatch(target, method, args = [])
    call = DispatcherHelpers.call_for(target, method, args)
    DispatcherHelpers.reify(Kobako::Transport::Dispatcher.dispatch(call, @services, @handles,
                                                                   Kobako::Transport::BlockFailure.new,
                                                                   ->(_bytes) { raise "no block" }))
  end

  # @behavior T-297
  def test_a_permissive_predicate_reaches_no_method_ruby_defines
    ROWS.each do |target, name|
      resp = dispatch(target, name, [:@x])
      assert_equal [false, "undefined"], [resp.ok?, (resp.payload.type unless resp.ok?)],
                   "#{name} on #{target} through guest dispatch with a predicate permitting every name " \
                   "must be refused as undefined"
    end
  end

  # The forwarded name the target defines is what the forwarder exists for.
  # @behavior T-256
  def test_a_permissive_forwarder_still_forwards_its_targets_own_method
    assert_equal [true, "inner"], dispatch("Cfg::Forward", "hello").then { |resp| [resp.ok?, resp.payload] },
                 "a name the target defines through a forwarder narrowing itself must reach the target"
  end
end
