# frozen_string_literal: true

require "test_helper"

# E2E — an entry of the Host App's own class whose endpoint text is not a
# Symbol or a String, read when an invocation composes the network.
class TestE2ENetworkEntryText < Minitest::Test
  include E2eGuestHelper

  # An entry answering only endpoints, each any object with the fields read.
  DuckEntry = Struct.new(:endpoints)
  HostAndPort = Struct.new(:host, :port)
  RuledEndpoint = Struct.new(:host, :port, :rules)

  # A rule of the Host App's own class; a Struct member named +method+ would
  # shadow Object#method.
  class DuckRule
    attr_reader :method, :path

    def initialize(method, path)
      @method = method
      @path = path
    end

    def allow? = true
  end

  # @behavior NW-216
  def test_a_duck_typed_endpoint_whose_text_is_another_class_raises_type_error
    {
      "host" => HostAndPort.new(123, 443),
      "rule path" => RuledEndpoint.new("x.example", 443, [DuckRule.new("GET", 42)])
    }.each do |field, endpoint|
      sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM).network(:duck, DuckEntry.new([endpoint]))

      assert_raises(TypeError,
                    "Sandbox#network with an endpoint object whose #{field} is an Integer must raise TypeError at " \
                    "the invocation") { sandbox.eval("1") }
    end
  end
end
