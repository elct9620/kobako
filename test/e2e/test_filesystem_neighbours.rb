# frozen_string_literal: true

require "test_helper"
require "tmpdir"

# E2E — a run beside four others that create, move, list, and write in the
# same filesystem, under both gvl: modes: the run still finishes its own
# lookups and creates. How long it takes is the host's: under :hold each call
# giving up the GVL waits behind every attacker's time slice, and how often
# one does depends on the cores to spare. So the test judges by what the run
# answers, and by each attacker, in its endless loop before the run began,
# still being there as it ends; a timeout only ends a run that jammed.
class TestE2EFilesystemNeighbours < Minitest::Test
  include E2eGuestHelper

  GVL_MODES = %i[hold release].freeze
  ROUNDS = 10
  # Far past any slowdown, so reaching it means the run jammed.
  MAX_RUN_TIME = 60

  # What every attacker asks between rounds, and how the test knows each one
  # is in its loop.
  class Crowd
    attr_reader :going

    def initialize
      @started = Thread::Queue.new
      @going = true
    end

    def mark_started = (@started << true) && nil

    def stop = @going = false

    def wait_started(count)
      count.times do
        raise "an attacker never reached its loop; its run failed first" unless @started.pop(timeout: 10)
      end
    end
  end

  # One round of each kind of attack, by the attacker's index.
  ATTACKS = [
    'File.write("/s/x%<i>d", "1"); File.delete("/s/x%<i>d")',
    'File.rename("/s/d/f%<i>d", "/s/e/f%<i>d"); File.rename("/s/e/f%<i>d", "/s/d/f%<i>d")',
    'Dir.children("/s"); Dir.children("/s/d")',
    'File.write("/s/w%<i>d", "x" * 4096)'
  ].freeze

  # An attacker going round until told to stop, counting its rounds. What one
  # round raises is the attacker's own, so it goes on.
  ATTACKER = <<~RUBY
    round = -> { begin; %<round>s; rescue SystemCallError; end }
    round.call
    Crowd::Ctl.mark_started
    n = 1
    while Crowd::Ctl.going
      round.call
      n += 1
    end
    n
  RUBY

  VICTIM = <<~RUBY.freeze
    done = 0
    #{ROUNDS}.times do
      File.exist?("/s/absent")
      File.write("/s/mine", "1")
      File.delete("/s/mine")
      done += 1
    end
    done
  RUBY

  # @behavior MT-064
  def test_a_run_beside_others_hammering_a_shared_filesystem_finishes
    answers = kinds.keys.product(GVL_MODES).map do |kind, mode|
      [kind, mode, *run_beside_attackers(kinds[kind].call, mode)]
    end

    assert_equal(kinds.keys.product(GVL_MODES).map { |kind, mode| [kind, mode, ROUNDS, true, true] }, answers,
                 "a run looking up and creating names beside four runs creating, moving, listing, and " \
                 "writing in the same Filesystem must answer every call, each attacker still going round " \
                 "as it ends and none failing")
  end

  private

  def kinds
    @kinds ||= {
      tmpfs: -> { Kobako::Filesystem::Tmpfs.new(seed_tree) },
      bind: -> { Kobako::Filesystem::Bind.new(make_host_dir) },
      overlay: lambda {
        Kobako::Filesystem::Overlay.new(lower: [Kobako::Filesystem::Tmpfs.new(seed_tree)],
                                        upper: Kobako::Filesystem::Tmpfs.new)
      }
    }
  end

  def seed_tree = (0...8).to_h { |i| ["d/f#{i}", "a"] }.merge("e/keep" => "k")

  def make_host_dir
    dir = Dir.mktmpdir("kobako-neighbours")
    (@host_dirs ||= []) << dir
    seed_tree.each do |path, contents|
      FileUtils.mkdir_p(File.join(dir, File.dirname(path)))
      File.write(File.join(dir, path), contents)
    end
    dir
  end

  def teardown
    Array(@host_dirs).each { |dir| FileUtils.rm_rf(dir) }
    super
  end

  # What the victim answers beside one attacker of each kind, whether every
  # attacker was still in its loop as the victim ended, and whether every one
  # went round, so none failed and was counted as done. A victim reaching its
  # timeout raises, the one way this test reads a jam.
  def run_beside_attackers(filesystem, mode)
    crowd = Crowd.new
    attackers = start_attackers(filesystem, mode, crowd)
    crowd.wait_started(attackers.size)
    answer = sandbox(filesystem, mode, MAX_RUN_TIME).eval(VICTIM).value
    there = attackers.all?(&:alive?)
    crowd.stop
    [answer, there, attackers.map(&:value).all? { |rounds| rounds.is_a?(Integer) && rounds.positive? }]
  end

  # One attacker of each kind, each on its own thread, outlasting the
  # victim's timeout so a jammed victim never finds them gone.
  def start_attackers(filesystem, mode, crowd)
    Array.new(ATTACKS.size) do |i|
      script = format(ATTACKER, round: format(ATTACKS[i], i: i))
      Thread.new { sandbox(filesystem, mode, MAX_RUN_TIME * 2).bind("Crowd::Ctl", crowd).eval(script).value }
    end
  end

  def sandbox(filesystem, mode, timeout)
    Kobako::Sandbox.new(wasm_path: REAL_WASM, gvl: mode, timeout: timeout)
                   .mount("/s", filesystem, access: :read_write)
  end
end
