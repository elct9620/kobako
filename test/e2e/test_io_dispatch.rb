# frozen_string_literal: true

require "test_helper"

# E2E — the capture channels beside a dispatch through real mruby. A
# dispatch travels on its own channel, so stdout and stderr hold exactly
# what guest code wrote around it and no byte of the exchange.
class TestE2EIoDispatch < Minitest::Test
  include E2eGuestHelper

  DISPATCH_BETWEEN_WRITES = <<~RUBY
    puts "before"
    $stderr.puts "aside"
    Echo::Identity.call("payload")
    puts "after"
  RUBY

  # @behavior S-153
  def test_a_dispatch_leaves_no_bytes_on_either_capture
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("Echo::Identity", ->(value) { value })

    execution = sandbox.eval(DISPATCH_BETWEEN_WRITES)

    assert_equal %W[before\nafter\n aside\n], [execution.stdout, execution.stderr],
                 "stdout and stderr through #eval must hold only what guest code wrote, " \
                 "carrying no protocol bytes from a dispatch made between the writes"
  end
end
