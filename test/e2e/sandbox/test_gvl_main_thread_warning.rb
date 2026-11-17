# frozen_string_literal: true

require "test_helper"

# E2E — the warning a run releasing the GVL gives on the main Thread, through
# the real Guest Binary.
#
# The warning is the process's one, so each case runs in a child whose
# stderr it reads back: a run on another Thread first, then two on the main
# Thread and one more on another Thread.
class TestE2EGvlMainThreadWarning < Minitest::Test
  include E2eGuestHelper
  include ChildProcess

  CHILD = <<~RUBY
    require "kobako"
    require "stringio"
    $stderr = StringIO.new
    sandbox = Kobako::Sandbox.new(wasm_path: ENV.fetch("KOBAKO_WASM"), gvl: :release)
    Thread.new { sandbox.eval("1") }.join
    off_main = $stderr.string.scan("main Thread").size
    2.times { sandbox.eval("1") }
    Thread.new { sandbox.eval("1") }.join
    print [off_main, $stderr.string.scan("main Thread").size].inspect
  RUBY

  # @behavior S-220
  def test_a_run_releasing_the_gvl_on_the_main_thread_warns_once
    assert_equal "[0, 1]", run_ruby(CHILD, env: { "KOBAKO_WASM" => REAL_WASM }, deadline: 20),
                 "Sandbox#eval with gvl: :release must warn nothing on another Thread, and warn once over two " \
                 "runs on the main Thread and one more on another Thread"
  end
end
