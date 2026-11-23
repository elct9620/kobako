# frozen_string_literal: true

require "test_helper"

# E2E — a run keeping the GVL whose guest never ends, beside another Thread
# that needs the GVL. The run's caller hands the GVL over each quantum while it
# waits for its guest, so the other Thread finishes while the guest still runs.
#
# Each case is a child, as a guest that never ends outlives any test; the
# child leaves with +exit!+ once it printed. The hog's guest writes a file on
# a Bind mount as it starts its endless part, and the child's main Thread waits
# for that file, so the other Thread asks for the GVL only once the guest is
# past every callback.
class TestE2EGvlTurns < Minitest::Test
  include E2eGuestHelper
  include ChildProcess

  DEADLINE = 30

  # The hogs, each a guest writing the start file and then never ending in its
  # own way. A nested run or a guest block writes the file from where it never
  # ends, so no callback is left for the caller to switch Threads in.
  HOGS = {
    compute: 'File.write("/t/in", "1"); loop { }',
    calls: 'File.write("/t/in", "1"); loop { Host::Nop.call }',
    blocks: 'Host::Each.call { File.write("/t/in", "1") }',
    nested: "Host::Nest.call",
    block: 'Host::Run.call { File.write("/t/in", "1"); loop { } }'
  }.freeze

  # Prints what the main Thread answers once the hog's guest has started.
  CHILD = <<~RUBY
    require "kobako"
    require "tmpdir"
    hog_gvl, hog, victim, victim_gvl = ARGV
    dir = Dir.mktmpdir
    wasm = ENV.fetch("KOBAKO_WASM")
    mounted = ->(sandbox) { sandbox.mount("/t", Kobako::Filesystem::Bind.new(dir), access: :read_write) }
    inner = mounted.(Kobako::Sandbox.new(wasm_path: wasm, gvl: :hold, timeout: nil))
    sandbox = mounted.(Kobako::Sandbox.new(wasm_path: wasm, gvl: hog_gvl.to_sym, timeout: nil))
    sandbox.bind("Host::Nop", -> {})
    sandbox.bind("Host::Each", ->(&block) { loop { block.call } })
    sandbox.bind("Host::Nest", -> { inner.eval('File.write("/t/in", "1"); loop { }') })
    sandbox.bind("Host::Run", ->(&block) { block.call })
    Thread.new { sandbox.eval(ENV.fetch("HOG")) }
    sleep 0.01 until File.exist?(File.join(dir, "in"))
    answer = case victim
             when "ruby" then Thread.new { 1 + 1 }.value
             when "run" then Kobako::Sandbox.new(wasm_path: wasm, gvl: victim_gvl.to_sym, timeout: 1).eval("1 + 1").value
             when "service"
               neighbour = Kobako::Sandbox.new(wasm_path: wasm, gvl: victim_gvl.to_sym, timeout: 1)
               neighbour.bind("Host::Two", -> { 2 })
               neighbour.eval("Host::Two.call").value
             end
    print answer
    $stdout.flush
    exit!(0)
  RUBY

  %i[hold release].product(%i[hold release]).each do |hog_gvl, victim_gvl|
    # @behavior S-243
    define_method(:"test_a_run_beside_a_#{hog_gvl}_guest_that_never_ends_answers_under_#{victim_gvl}") do
      assert_equal "2", beside(:compute, "run", hog_gvl: hog_gvl, victim_gvl: victim_gvl),
                   "Sandbox#eval with gvl: :#{victim_gvl} and timeout: 1 must answer while a Sandbox with " \
                   "gvl: :#{hog_gvl} on another Thread runs `loop { }` without a deadline"
    end
  end

  # @behavior S-243
  def test_a_run_whose_service_returns_beside_a_guest_that_never_ends_answers
    assert_equal "2", beside(:compute, "service"),
                 "Sandbox#eval with timeout: 1 whose script calls a Service must answer while a Sandbox " \
                 "keeping the GVL on another Thread runs `loop { }` without a deadline"
  end

  # @behavior S-243
  def test_a_ruby_thread_beside_a_guest_that_never_ends_finishes
    assert_equal "2", beside(:compute, "ruby"),
                 "a plain Ruby Thread must finish while a Sandbox keeping the GVL on another Thread runs " \
                 "`loop { }` without a deadline"
  end

  {
    calls: ["calls_a_service_without_end", "calls an empty Service without end"],
    blocks: ["has_a_service_call_its_block_without_end", "has a Service call its block without end"],
    nested: ["has_a_service_run_a_nested_run_that_never_ends", "has a Service run a nested Sandbox#eval of `loop { }`"],
    block: ["has_a_service_call_its_block_that_never_ends", "has a Service call its block, which runs `loop { }`"]
  }.each do |hog, (name, does)|
    # @behavior S-243
    define_method(:"test_a_run_beside_a_guest_that_#{name}_answers") do
      assert_equal "2", beside(hog, "run"),
                   "Sandbox#eval with timeout: 1 must answer while a Sandbox keeping the GVL on another " \
                   "Thread #{does}, without a deadline"
    end
  end

  private

  # What the main Thread answers once the hog's guest started, or
  # +:deadlocked+.
  def beside(hog, victim, hog_gvl: :hold, victim_gvl: :hold)
    run_ruby(CHILD, hog_gvl.to_s, hog.to_s, victim, victim_gvl.to_s,
             env: { "KOBAKO_WASM" => REAL_WASM, "HOG" => HOGS.fetch(hog) }, deadline: DEADLINE)
  end
end
