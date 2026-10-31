# frozen_string_literal: true

require "test_helper"

# Differential parity — a Tmpfs mounted at a guest path: both frontends
# hand the guest the same files, and a write beneath a read-only Mount
# fails under the same attribution.
class TestParityMount < Parity::Case
  # @behavior MT-001
  def test_a_mounted_file_reads_alike
    assert_parity Parity::Scenario.new(
      name: "mount-read",
      mounts: [{ at: "/data", files: { "note.txt" => "hello" }, access: "read" }],
      invocations: [{ verb: "eval", source: 'File.read("/data/note.txt")' }]
    )
  end

  # @behavior FL-006
  def test_a_write_beneath_a_read_only_mount_fails_alike
    assert_parity Parity::Scenario.new(
      name: "mount-read-only-write",
      mounts: [{ at: "/data", files: {}, access: "read" }],
      invocations: [{ verb: "eval", source: 'File.write("/data/new.txt", "x")' }]
    )
  end

  # The inode number is a hash drawn per process, so each frontend's own is
  # compared with itself, and only that comparison crosses.
  # @behavior FL-038
  def test_a_report_reads_alike
    assert_parity Parity::Scenario.new(
      name: "mount-file-stat",
      mounts: [{ at: "/data", files: { "note.txt" => "hello", "dir/inner" => "x" }, access: "read" }],
      invocations: [{ verb: "eval", source: <<~RUBY }]
        stat = File.stat("/data/note.txt")
        [stat.size, stat.nlink, stat.ftype, stat.file?, File.ftype("/data/dir"), File.size?("/data/none"),
         stat.ino == File.lstat("/data/note.txt").ino, stat.ino == File.stat("/data/dir").ino]
      RUBY
    )
  end
end
