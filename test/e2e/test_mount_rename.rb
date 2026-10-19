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
    secrets = Kobako::Filesystem::Memory.new("app/.env" => "secret", "app/main.rb" => "ok")
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

  # Without a block the move judges no path the block could have refused.
  def test_a_directory_beneath_a_mount_without_a_block_moves
    secrets = Kobako::Filesystem::Memory.new("app/.env" => "secret")
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM).mount("/data", secrets, access: :read_write)

    assert_equal [:moved, "secret"], sandbox.eval(RENAME_THEN_READ).value,
                 "File.rename of a directory beneath a Sandbox#mount without a block must move it"
  end
end
