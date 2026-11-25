# frozen_string_literal: true

require "test_helper"

# E2E — a Tmpfs the Host App lets go, through the real Guest Binary: what its
# files hold counts toward Ruby's own decision to collect, so dropped
# instances go without the Host App calling GC.start. Each case runs in a
# child collecting by thresholds of its own.
class TestE2EMountCollected < Minitest::Test
  include CollectionChild

  RUNS = 40

  # Write until the Tmpfs at +/t+ is full.
  FILL = 'c = "x" * 65_536; begin; File.open("/t/f", "w") { |f| loop { f.write(c) } }; rescue Errno::ENOSPC; end'

  # How many of RUNS Tmpfs instances, each filled by one run under +gvl+ and
  # let go, are alive once every run ended. Each is made in a frame of its
  # own, so none stays on the stack the count is read from.
  DROPPED = <<~RUBY.freeze
    require "weakref"
    def fill_and_drop(sandbox)
      tmpfs = Kobako::Filesystem::Tmpfs.new
      sandbox.eval(#{FILL.inspect}) { |ctx| ctx.mount("/t", tmpfs, access: :read_write) }
      WeakRef.new(tmpfs)
    end
    sandbox = Kobako::Sandbox.new(wasm_path: WASM, gvl: ARGV.fetch(0).to_sym)
                             .mount("/t", Kobako::Filesystem::Tmpfs.new, access: :read_write)
    refs = Array.new(#{RUNS}) { fill_and_drop(sandbox) }
    print refs.count(&:weakref_alive?)
  RUBY

  # @behavior MT-066
  def test_a_dropped_tmpfs_is_collected_without_being_asked
    %w[hold release].each do |gvl|
      alive = run_collection_child(DROPPED, gvl, thresholds: BY_REPORTED_MEMORY).to_i

      assert_operator alive, :<, RUNS / 2,
                      "#{RUNS} Tmpfs instances each filled through Sandbox#eval (gvl: #{gvl}) and let go " \
                      "must mostly be collected by Ruby on its own, with no GC.start"
    end
  end

  # How many times Ruby collected while a run filled its Tmpfs.
  FILLED = <<~RUBY.freeze
    sandbox = Kobako::Sandbox.new(wasm_path: WASM)
                             .mount("/t", Kobako::Filesystem::Tmpfs.new, access: :read_write)
    sandbox.eval("nil")
    before = GC.count
    sandbox.eval(#{FILL.inspect})
    print GC.count - before
  RUBY

  # No run reaches the child's thresholds, so any collection during it was
  # asked for.
  # @behavior MT-067
  def test_a_tmpfs_that_fills_collects_no_garbage
    assert_equal "0", run_collection_child(FILLED, thresholds: PAST_ANY_RUN),
                 "a Tmpfs filled to Errno::ENOSPC through Sandbox#eval must leave GC.count as it was"
  end
end
