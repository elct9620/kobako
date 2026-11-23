# frozen_string_literal: true

require "test_helper"

# E2E — the host directories Binds keep for walks, against a process out of
# descriptors, each case a child of its own.
class TestE2EHostDescriptors < Minitest::Test
  include DescriptorChild

  # @behavior FS-151
  def test_binds_reading_many_directories_leave_the_host_its_descriptors
    room = run_descriptor_child(<<~RUBY)
      roots = (0...4).to_h { |at| [:"b\#{at}", tree("b\#{at}", count: 100)] }
      before = room
      sandbox(**roots).eval('4.times { |b| 100.times { |d| File.read("/b" + b.to_s + "/d" + d.to_s + "/note.txt") } }')
      print [before, room].inspect
    RUBY
    before, after = room.scan(/\d+/).map(&:to_i)
    assert_operator after, :>=, before - 64,
                    "Sandbox#eval reading 100 directories beneath each of 4 Binds must leave the Host App all " \
                    "but 64 of the #{before} files it could open, leaving #{after}"
  end

  # @behavior FS-217
  def test_a_bind_reads_while_another_binds_directories_fill_the_host
    answer = run_descriptor_child(<<~RUBY)
      full = sandbox(full: tree("full", count: 100))
      full.eval('100.times { |d| File.read("/full/d" + d.to_s + "/note.txt") }')
      fresh = sandbox(fresh: tree("fresh"))
      leave(0)
      print answer(fresh, 'File.read("/fresh/d0/note.txt")')
    RUBY
    assert_equal "hello", answer,
                 "File.read beneath a Bind while another Bind's directories fill the process must read the file"
  end

  HELD_OPEN = <<~GUEST
    held = Dir.new("/b0/d0")
    read = (1...100).all? { |d| File.read("/b0/d" + d.to_s + "/note.txt") == "hello" }
    [read, held.children]
  GUEST

  # @behavior FS-220
  def test_a_directory_held_open_answers_after_the_kept_ones_give_way
    answer = run_descriptor_child(<<~RUBY)
      sandboxed = sandbox(b0: tree("b0", count: 100))
      leave(3)
      print answer(sandboxed, #{HELD_OPEN.inspect}).inspect
    RUBY
    assert_equal [true, ["note.txt"]].inspect, answer,
                 "Dir#children on a directory held open while reads in 99 others leave the host 3 descriptors " \
                 "must list the directory, every read landing"
  end

  # Descriptors the process holds before and after its Binds kept 100
  # directories, and in a child forked after that.
  FORKED = <<~RUBY
    held = -> { Dir.children("/dev/fd").size }
    sandboxed = sandbox(b0: tree("b0", count: 100))
    before = held.call
    sandboxed.eval('100.times { |d| File.read("/b0/d" + d.to_s + "/note.txt") }')
    kept = held.call
    reader, writer = IO.pipe
    pid = fork { reader.close; writer.print(held.call); exit!(0) }
    writer.close
    child = reader.read.to_i
    Process.wait(pid)
    print [before, kept, child].inspect
  RUBY

  # @behavior FS-231
  def test_a_forked_child_inherits_none_of_the_directories_binds_keep
    skip "this platform cannot fork" unless Process.respond_to?(:fork)
    before, kept, child = run_descriptor_child(FORKED).scan(/\d+/).map(&:to_i)
    assert_operator kept, :>, before + 32, "a Bind reading 100 directories must keep some of them open"
    assert_operator child, :<=, before + 2,
                    "a child forked after a Bind kept #{kept - before} directories must hold none of them, " \
                    "holding #{child} descriptors against #{before} before"
  end

  DEEP = "d/" * 31

  OPERATIONS = {
    read: %{File.read("#{DEEP}note.txt")},
    create: %{File.write("#{DEEP}new.txt", "x")},
    stat: %{File.stat("#{DEEP}note.txt").size},
    list: %{Dir.children("#{DEEP}")},
    rename: %{File.rename("#{DEEP}note.txt", "#{DEEP}moved.txt")},
    delete: %{File.delete("#{DEEP}note.txt")}
  }.freeze

  # @behavior FS-223
  def test_each_operation_32_directories_deep_lands_with_three_descriptors_free
    answers = OPERATIONS.to_h do |name, call|
      [name, run_descriptor_child(<<~RUBY)]
        sandboxed = sandbox(b0: tree("b0", depth: 32))
        leave(3)
        print answer(sandboxed, #{%(Dir.chdir("/b0/d0") { #{call}; :done }).inspect})
      RUBY
    end
    assert_equal OPERATIONS.keys.to_h { |name| [name, "done"] }, answers,
                 "each operation on a file 32 directories deep beneath a Bind must land with 3 host descriptors free"
  end
end
