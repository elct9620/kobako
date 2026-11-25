# frozen_string_literal: true

require "test_helper"

# E2E — a listing of a Filesystem written in Ruby stops between its stats once
# the run is past its timeout, so a slow stat holds the directory no longer
# than the one call.
class TestE2ERubyFilesystemListing < Minitest::Test
  include E2eGuestHelper

  # Room for the guest to reach its first stat.
  TIMEOUT = 0.2

  Stat = Data.define(:directory, :size, :ino) do
    def directory? = directory
  end

  # The files +a+, +b+, and +c+ at its root, the stat of +a+ taking as long
  # as the run's timeout from its own call, past the timeout that began
  # before it, each stat of a file recorded.
  class SlowFirstStat
    attr_reader :asked

    def initialize
      @asked = []
    end

    def open(path, _flags) = raise(Errno::ENOENT, path)

    def children(_path) = %w[a b c]

    def stat(path)
      return Stat.new(directory: true, size: 0, ino: 1) if path.empty?

      @asked << path
      sleep TIMEOUT if path == "a"
      Stat.new(directory: false, size: 1, ino: path.ord)
    end
  end

  # @behavior RT-425
  def test_a_listing_past_its_timeout_asks_no_further_stat
    filesystem = SlowFirstStat.new
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM, timeout: TIMEOUT).mount("/", filesystem)

    assert_raises(Kobako::TimeoutError, "a guest listing past its timeout must raise Kobako::TimeoutError") do
      sandbox.eval('Dir.children("/")')
    end
    assert_equal %w[a], filesystem.asked,
                 "a guest listing a Ruby Filesystem whose first stat outlasts the timeout must ask no further stat"
  end
end
