# frozen_string_literal: true

require "test_helper"

# E2E — a rename beneath a Sandbox#mount, reached through the real Guest
# Binary's File.
class TestE2EMountRename < Minitest::Test
  include E2eGuestHelper

  # Rename +app+, then read what its +.env+ would be beneath the new name.
  RENAME_THEN_READ = <<~RUBY
    moved = begin; File.rename("/data/app", "/data/app2"); :moved; rescue Errno::EXDEV; :refused; end
    read = begin; File.read("/data/app2/.env"); rescue Errno::ENOENT; :missing; end
    [moved, read]
  RUBY

  # A read-write mount of +app/+ whose block refuses only +app/.env+.
  def sandbox_refusing_env
    secrets = Kobako::Filesystem::Tmpfs.new("app/.env" => "secret", "app/main.rb" => "ok")
    Kobako::Sandbox.new(wasm_path: REAL_WASM).mount("/data", secrets, access: :read_write) do |path, _access|
      path != "/data/app/.env"
    end
  end

  # The block refuses only +.env+ by its path, so renaming the directory
  # above it would hand the guest a path the block never refused.
  # @behavior MT-018
  def test_a_directory_beneath_a_mount_with_a_block_does_not_move
    assert_equal %i[refused missing], sandbox_refusing_env.eval(RENAME_THEN_READ).value,
                 "File.rename of a directory beneath a Sandbox#mount with a block must raise Errno::EXDEV " \
                 "and leave what the block refuses unreachable"
  end

  # Each mount is a Filesystem of its own, so a rename between them would
  # cross devices, as Linux refuses it.
  # @behavior MT-027
  def test_a_rename_from_one_mount_into_another_raises_exdev
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
                             .mount("/data", Kobako::Filesystem::Tmpfs.new, access: :read_write)
                             .mount("/data/inner", Kobako::Filesystem::Tmpfs.new("note" => "x"), access: :read_write)
    script = <<~RUBY
      moved = begin; File.rename("/data/inner/note", "/data/note"); :moved; rescue Errno::EXDEV; :refused; end
      [moved, File.exist?("/data/inner/note"), File.exist?("/data/note")]
    RUBY

    assert_equal [:refused, true, false], sandbox.eval(script).value,
                 "File.rename from a Sandbox#mount into the one it is nested in must raise Errno::EXDEV " \
                 "and leave the file where it was"
  end

  # Without a block the move judges no path the block could have refused.
  def test_a_directory_beneath_a_mount_without_a_block_moves
    secrets = Kobako::Filesystem::Tmpfs.new("app/.env" => "secret")
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM).mount("/data", secrets, access: :read_write)

    assert_equal [:moved, "secret"], sandbox.eval(RENAME_THEN_READ).value,
                 "File.rename of a directory beneath a Sandbox#mount without a block must move it"
  end
end
