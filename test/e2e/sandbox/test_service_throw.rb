# frozen_string_literal: true

require "test_helper"

# E2E — a Service's +throw+ reaching the Host App's +catch+ through the real
# Guest Binary, past Ruby code the run calls on its way out.
#
# A +throw+ that loses its target crashes the process, so the run is a child
# process and a crash fails through its status.
class TestE2EServiceThrow < Minitest::Test
  include E2eGuestHelper
  include ChildProcess

  DEADLINE = 60

  CHILD = <<~RUBY
    require "kobako"
    Stat = Data.define(:directory, :size, :ino) { def directory? = directory }
    $closed = 0
    file = Class.new do
      def pread(length, offset) = "hello".byteslice(offset, length)
      def size = 5
      def close = (catch(:inner) { throw :inner }; $closed += 1)
    end
    filesystem = Class.new do
      define_method(:open) { |_path, _flags| file.new }
      def stat(path) = path.empty? ? Stat.new(true, 0, 1) : Stat.new(false, 5, 2)
    end
    sandbox = Kobako::Sandbox.new(wasm_path: ENV.fetch("KOBAKO_WASM"), gvl: ARGV[0].to_sym)
                             .mount("/data", filesystem.new, access: :read)
    sandbox.bind("Host::Throw", -> { throw :out, 42 })
    caught = catch(:out) { sandbox.eval('f = File.open("/data/note.txt"); f.read(1); Host::Throw.call'); :not_thrown }
    print caught, " closed=", $closed
  RUBY

  %i[hold release].each do |gvl|
    # @behavior S-221
    define_method(:"test_a_services_throw_reaches_its_catch_past_a_close_that_catches_its_own_under_#{gvl}") do
      assert_equal "42 closed=1", run_ruby(CHILD, gvl.to_s, env: { "KOBAKO_WASM" => REAL_WASM }, deadline: DEADLINE),
                   "a Service's throw :out, 42 must reach the Host App's catch(:out) around Sandbox#eval with " \
                   "gvl: :#{gvl}, while the guest's open file on a Ruby Filesystem whose close runs its own " \
                   "catch and throw is closed once"
    end
  end
end
