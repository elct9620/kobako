# frozen_string_literal: true

require "test_helper"

# E2E — a capability gem calling back into guest code mid-operation, on the
# JSON surface: a guest-defined serialization hook that raises during
# generation. The raise has to stay a guest exception the caller may
# rescue, never a trap that retires the Sandbox.
class TestJsonRaisingHook < Minitest::Test
  include JsonGuestHelper

  RAISING_HOOK = <<~RUBY
    class C; def as_json; raise "no serial form"; end; end
    begin
      JSON.generate([C.new])
    rescue RuntimeError => e
      e.message
    end
  RUBY

  # @behavior MR-013
  def test_a_raising_serialization_hook_is_rescuable
    assert_equal "no serial form", eval_json(RAISING_HOOK),
                 "a raise inside a guest-defined as_json during JSON.generate must be a guest " \
                 "exception the caller may rescue, never a trap"
  end
end
