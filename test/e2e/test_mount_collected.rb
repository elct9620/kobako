# frozen_string_literal: true

require "test_helper"
require "weakref"

# E2E — a Tmpfs the Host App lets go, through the real Guest Binary: what its
# files hold counts toward Ruby's own decision to collect, so dropped
# instances go without the Host App calling GC.start.
class TestE2EMountCollected < Minitest::Test
  include E2eGuestHelper

  RUNS = 40

  # Write until the Tmpfs at +/t+ is full.
  FILL = 'c = "x" * 65_536; begin; File.open("/t/f", "w") { |f| loop { f.write(c) } }; rescue Errno::ENOSPC; end'

  # WeakRefs to each Tmpfs that one of RUNS runs under +gvl+ filled and let go.
  def dropped_after_filling(gvl)
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM, gvl: gvl)
                             .mount("/t", Kobako::Filesystem::Tmpfs.new, access: :read_write)
    Array.new(RUNS) do
      tmpfs = Kobako::Filesystem::Tmpfs.new
      sandbox.eval(FILL) { |ctx| ctx.mount("/t", tmpfs, access: :read_write) }
      WeakRef.new(tmpfs)
    end
  end

  # @behavior MT-066
  def test_a_dropped_tmpfs_is_collected_without_being_asked
    %i[hold release].each do |gvl|
      alive = dropped_after_filling(gvl).count(&:weakref_alive?)

      assert_operator alive, :<, RUNS / 2,
                      "#{RUNS} Tmpfs instances each filled through Sandbox#eval (gvl: #{gvl}) and let go " \
                      "must mostly be collected by Ruby on its own, with no GC.start"
    end
  end
end
