# frozen_string_literal: true

require "test_helper"

# E2E — the root every Sandbox#mount is reached from, through the real Guest
# Binary: the guest walks from `/` as Linux walks from a process's root, and
# where the Host App mounts nothing there, the root holds only the way to
# each mount.
class TestE2EMountRoot < Minitest::Test
  include E2eGuestHelper

  # @behavior MT-060
  def test_the_root_holds_only_the_way_to_each_mount
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
                             .mount("/data", Kobako::Filesystem::Tmpfs.new)
                             .mount("/opt/app", Kobako::Filesystem::Tmpfs.new)
    script = '[Dir.children("/").sort, Dir.children("/opt"), ' \
             'begin; File.write("/x", "1"); :wrote; rescue Errno::EROFS; :erofs; end]'

    assert_equal [%w[data opt], %w[app], :erofs], sandbox.eval(script).value,
                 "with no Sandbox#mount at /, Dir.children must list only the way to each mount and a write raise EROFS"
  end

  # @behavior MT-061
  def test_a_dotdot_out_of_a_mount_at_the_root_climbs_into_the_root
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
                             .mount("/data", Kobako::Filesystem::Tmpfs.new)
                             .mount("/tmp", Kobako::Filesystem::Tmpfs.new("y" => "tmp-y"))
    script = '[File.read("/data/../tmp/y"), (Dir.chdir("/data"); File.read("../tmp/y"))]'

    assert_equal %w[tmp-y tmp-y], sandbox.eval(script).value,
                 "File.read through a `..` out of one Sandbox#mount must reach a sibling mount, as Linux's does"
  end

  # @behavior MT-062
  def test_a_mount_at_the_root_is_the_root
    root = Kobako::Filesystem::Tmpfs.new("data/.keep" => "", "etc/.keep" => "")
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
                             .mount("/", root, access: :read_write)
                             .mount("/data", Kobako::Filesystem::Tmpfs.new)
    script = 'list = Dir.children("/").sort; File.write("/x", "1"); list'

    assert_equal [%w[data etc], "1"], [sandbox.eval(script).value, sandbox.eval('File.read("/x")').value],
                 "a Sandbox#mount at / must be the root the guest lists and writes"
  end
end
