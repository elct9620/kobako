# frozen_string_literal: true

require "test_helper"

# E2E — what a script reaches through a +Dir+ it opened beneath a Mount. Each
# literal is what CRuby 3.4 on Linux answers for the same entries.
class TestE2EDirOpened < Minitest::Test
  include E2eGuestHelper

  # The class each call raises, or what it answers.
  ATTEMPT = "attempt = ->(call) { begin; call.call; rescue Exception => e; e.class.to_s; end }\n"

  OPENED = <<~RUBY
    dir = Dir.open("/m/d")
    read = [dir.path, dir.read, dir.read, dir.read, dir.read, dir.read]
    each = []
    answer = dir.each { |name| each << name }
    dir.close
    [read, each, answer.equal?(dir), Dir.open("/m/d") { |opened| opened.class.to_s }]
  RUBY

  CLOSED = <<~RUBY
    dir = Dir.open("/m/d")
    dir.close
    [dir.close, attempt.(-> { dir.read })]
  RUBY

  LISTED_AGAIN = <<~RUBY
    Dir.open("/m/d") do |dir|
      each = []
      [dir.children.sort, (dir.each_child { |name| each << name }).equal?(dir), each.sort, dir.read,
       dir.rewind.equal?(dir), dir.read]
    end
  RUBY

  NAMED = <<~RUBY
    dir = Dir.open("/m/d")
    named = [dir.inspect, dir.path]
    dir.close
    named << dir.path
  RUBY

  def sandbox
    filesystem = Kobako::Filesystem::Tmpfs.new("d/x" => "1", "d/y" => "2")
    Kobako::Sandbox.new(wasm_path: REAL_WASM).mount("/m", filesystem)
  end

  # @behavior FL-053
  def test_an_opened_directory_yields_its_entries_one_at_a_time
    read, each, same, block = sandbox.eval(OPENED).value

    assert_equal ["/m/d", ".", "..", %w[x y], nil], [*read.take(3), read[3, 2].sort, read[5]],
                 "Dir#read through the bundled guest must answer . and .., each name, and then nil"
    assert_equal [%w[. .. x y], true, "Dir"], [each.sort, same, block],
                 "Dir#each through the bundled guest must yield every entry and answer the Dir, and " \
                 "Dir.open with a block must answer the block's value"
  end

  # @behavior FL-054
  def test_a_closed_directory_raises_io_error
    assert_equal [nil, "IOError"], sandbox.eval(ATTEMPT + CLOSED).value,
                 "Dir#read of a closed directory through the bundled guest must raise IOError, and a " \
                 "second close must answer nil"
  end

  # @behavior FL-072
  def test_an_opened_directory_lists_its_children_again_and_rewinds
    assert_equal [%w[x y], true, %w[x y], nil, true, "."], sandbox.eval(LISTED_AGAIN).value,
                 "Dir#children, Dir#each_child, and Dir#rewind through the bundled guest must list the " \
                 "two names, leave the read at the end, and rewind it to ."
  end

  # @behavior FL-073
  def test_a_directory_names_its_path_open_or_closed
    assert_equal ["#<Dir:/m/d>", "/m/d", "/m/d"], sandbox.eval(NAMED).value,
                 "Dir#inspect and Dir#path through the bundled guest must name the path, and Dir#path " \
                 "must answer it after the close"
  end
end
