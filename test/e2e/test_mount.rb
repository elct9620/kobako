# frozen_string_literal: true

require "test_helper"

# E2E — Sandbox#mount and Context#mount through the real Guest Binary: a
# Filesystem the guest's File reaches beneath the mounted path, the
# registration rules a mount shares with bind, and a mount's block deciding
# each open.
class TestE2EMount < Minitest::Test
  include E2eGuestHelper

  def sandbox_with_note(&policy)
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.mount("/data", Kobako::Filesystem::Tmpfs.new("note.txt" => "hello"), &policy)
  end

  # What +script+ answers with +filesystem+ mounted at +/data+.
  def run_beneath(filesystem, script, access: :read)
    Kobako::Sandbox.new(wasm_path: REAL_WASM).mount("/data", filesystem, access: access).eval(script).value
  end

  # @behavior MT-001
  def test_a_mounted_filesystem_is_reachable_from_the_guest
    assert_equal "hello", sandbox_with_note.eval('File.read("/data/note.txt")').value,
                 "a file in a Tmpfs mounted through Sandbox#mount must be read by the guest's File"
  end

  # @behavior MT-002
  def test_a_mount_after_the_first_invocation_is_refused
    sandbox = sandbox_with_note
    sandbox.eval("nil")

    assert_raises(ArgumentError, "Sandbox#mount after the first invocation must be refused") do
      sandbox.mount("/more", Kobako::Filesystem::Tmpfs.new)
    end
  end

  # @behavior MT-003
  def test_a_mount_path_that_is_not_absolute_is_refused
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)

    ["data", "/data/../etc", "/data/", "/a//b"].each do |at|
      assert_raises(ArgumentError, "Sandbox#mount at #{at.inspect} must be refused as no absolute guest path") do
        sandbox.mount(at, Kobako::Filesystem::Tmpfs.new)
      end
    end
  end

  # An Overlay refuses the same object as a Layer, so every entry a
  # Filesystem is handed through answers alike.
  # @behavior MT-022
  def test_what_is_no_filesystem_is_refused_as_the_wrong_type
    tmpfs = Kobako::Filesystem::Tmpfs.new
    refused = [-> { Kobako::Sandbox.new(wasm_path: REAL_WASM).mount("/data", Object.new) },
               -> { Kobako::Filesystem::Overlay.new(lower: [Object.new], upper: tmpfs) }].map do |call|
      assert_raises(TypeError, "Sandbox#mount and Overlay.new of no Filesystem must raise TypeError", &call)
    end

    assert_equal 2, refused.size
  end

  # @behavior MT-004
  def test_a_path_mounted_twice_is_refused
    assert_raises(ArgumentError, "Sandbox#mount at a path already mounted must be refused") do
      sandbox_with_note.mount("/data", Kobako::Filesystem::Tmpfs.new)
    end
  end

  # @behavior MT-005
  def test_a_mount_nested_inside_another_is_refused
    assert_raises(ArgumentError, "Sandbox#mount beneath a mounted path must be refused") do
      sandbox_with_note.mount("/data/inner", Kobako::Filesystem::Tmpfs.new)
    end
  end

  # @behavior MT-007
  def test_a_block_refusing_an_open_denies_it_and_the_script_goes_on
    sandbox = sandbox_with_note { |_path, _access| false }
    script = 'begin; File.read("/data/note.txt"); rescue Errno::EACCES; "denied"; end'

    assert_equal "denied", sandbox.eval(script).value,
                 "an open a Sandbox#mount block refuses must reach the guest as Errno::EACCES"
  end

  # @behavior MT-025
  def test_a_block_is_asked_once_for_a_read_as_a_read
    asked = []
    sandbox = sandbox_with_note { |_path, access| asked << access }

    assert_equal ["hello", [:read]], [sandbox.eval('File.read("/data/note.txt")').value, asked],
                 "File.read beneath a Sandbox#mount block must ask the block once, as a read"
  end

  # @behavior MT-008
  def test_a_block_that_raises_refuses_the_open
    sandbox = sandbox_with_note { |_path, _access| raise "no" }
    script = 'begin; File.read("/data/note.txt"); rescue Errno::EACCES; "denied"; end'

    assert_equal "denied", sandbox.eval(script).value,
                 "an open whose Sandbox#mount block raises must reach the guest as Errno::EACCES"
  end

  # Compaction moves every movable object, so a block the ext reached
  # through a stale address would answer as some other object and refuse.
  # It runs from a Service call, while the block itself is off the stack.
  # @behavior MT-016
  def test_a_mounts_block_answers_after_the_host_compacts_its_heap
    skip "GC compaction unavailable on this Ruby build" unless GC.respond_to?(:verify_compaction_references)

    sandbox = sandbox_with_note { |path, _access| path == "/data/note.txt" }
    sandbox.bind("Host::Compact", -> { GC.verify_compaction_references(expand_heap: true, toward: :empty) && nil })
    script = 'a = File.read("/data/note.txt"); Host::Compact.call; a + File.read("/data/note.txt")'

    assert_equal "hellohello", sandbox.eval(script).value,
                 "a Sandbox#mount block must still allow an open after the host compacts its heap mid-run"
  end

  # @behavior MT-009
  def test_a_context_replaces_a_mount_for_its_invocation_only
    sandbox = sandbox_with_note
    replaced = sandbox.eval('File.read("/data/note.txt")') do |ctx|
      ctx.mount("/data", Kobako::Filesystem::Tmpfs.new("note.txt" => "other"))
    end.value

    assert_equal "other", replaced, "ctx.mount must replace the mount for its own invocation"
    assert_equal "hello", sandbox.eval('File.read("/data/note.txt")').value,
                 "the invocation after a ctx.mount must read the Sandbox's own mount"
  end

  # @behavior MT-010
  def test_a_context_cannot_mount_a_path_the_sandbox_never_mounted
    assert_raises(ArgumentError, "ctx.mount at a path the Sandbox never mounted must be refused") do
      sandbox_with_note.eval("nil") { |ctx| ctx.mount("/other", Kobako::Filesystem::Tmpfs.new) }
    end
  end

  # @behavior MT-011
  def test_an_overlay_reaches_the_guest_as_one_view
    lower = Kobako::Filesystem::Tmpfs.new("note.txt" => "hello")
    overlay = Kobako::Filesystem::Overlay.new(lower: [lower], upper: Kobako::Filesystem::Tmpfs.new)
    script = 'File.write("/data/note.txt", "changed"); File.read("/data/note.txt")'

    assert_equal "changed", run_beneath(overlay, script, access: :read_write),
                 "a file overwritten through an Overlay.new mount must read back what was written"
    assert_equal "hello", run_beneath(lower, 'File.read("/data/note.txt")'),
                 "the lower Tmpfs beneath an Overlay.new mount must still hold the file as it was"
  end
end
