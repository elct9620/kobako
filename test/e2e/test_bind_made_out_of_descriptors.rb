# frozen_string_literal: true

require "test_helper"

# E2E — Bind.new against a process out of descriptors, each case a child of
# its own. The Binds a Host App let go of hold their host directories until
# Ruby collects them, and Ruby collects as a File.open finds the process
# full, so garbage made before the process fills is gone by the time it is
# full. The garbage is made last instead, into room left for it.
class TestE2EBindMadeOutOfDescriptors < Minitest::Test
  include DescriptorChild

  # Thirty Binds let go of in the room left for them, which leaves one
  # descriptor, one short of what Bind.new needs.
  GARBAGE = <<~RUBY
    def garbage(root)
      leave(31)
      30.times { Kobako::Filesystem::Bind.new(root) }
    end
  RUBY

  # @behavior FS-226
  def test_a_bind_is_made_while_other_binds_directories_fill_the_host
    answer = run_descriptor_child(<<~RUBY)
      full = sandbox(full: tree("full", count: 100))
      full.eval('100.times { |d| File.read("/full/d" + d.to_s + "/note.txt") }')
      root = tree("fresh")
      leave(0)
      print Kobako::Filesystem::Bind.new(root).class
    RUBY
    assert_equal "Kobako::Filesystem::Bind", answer,
                 "Bind.new while another Bind's directories fill the process must make the Bind"
  end

  PER_REQUEST = <<~RUBY
    root = tree("b0")
    begin
      2000.times do
        Kobako::Sandbox.new(wasm_path: WASM).mount("/b0", Kobako::Filesystem::Bind.new(root)).eval("nil")
      end
      print :served
    rescue SystemCallError => e
      print e.class
    end
  RUBY

  # Ruby before 3.4 collects nothing for Dir.new while collection is disabled.
  DISABLED = RUBY_VERSION < "3.4" ? "Errno::EMFILE" : "served"

  # @behavior FS-227
  def test_a_bind_made_for_each_request_collects_the_ones_let_go
    answers = ["", "GC.disable\n"].map { |collection| run_descriptor_child(collection + PER_REQUEST) }
    assert_equal ["served", DISABLED], answers,
                 "Bind.new for each of 2000 requests must make each Bind, and with collection disabled answer " \
                 "as Dir.new does on this Ruby"
  end

  CALLBACK = <<~RUBY
    root = tree("b0")
    made = nil
    sandboxed = Kobako::Sandbox.new(wasm_path: WASM)
    sandboxed.mount("/data", Kobako::Filesystem::Tmpfs.new, access: :read_write) do |_path, access|
      made ||= Kobako::Filesystem::Bind.new(root).class if access == :read_write
      true
    end
    garbage(root)
    sandboxed.eval('File.write("/data/a", "1")')
    print made
  RUBY

  # @behavior FS-228
  def test_a_bind_made_in_a_callback_holding_a_directory_completes
    assert_equal "Kobako::Filesystem::Bind", run_descriptor_child(GARBAGE + CALLBACK),
                 "Bind.new in a Sandbox#mount block a File.write calls, the process out of descriptors, must make it"
  end

  # @behavior FS-229
  def test_a_bind_made_with_every_descriptor_in_use_raises_the_hosts_refusal
    answer = run_descriptor_child(<<~RUBY)
      root = tree("b0")
      leave(0)
      begin
        Kobako::Filesystem::Bind.new(root)
      rescue SystemCallError, ArgumentError => e
        print e.class
      end
    RUBY
    assert_equal "Errno::EMFILE", answer, "Bind.new with every host descriptor in use must raise Errno::EMFILE"
  end
end
