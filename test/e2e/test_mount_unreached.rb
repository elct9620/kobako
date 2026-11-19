# frozen_string_literal: true

require "test_helper"

# E2E — a Sandbox#mount nested inside another that kobako refuses to place,
# through the real Guest Binary: the run fails naming it, as a container fails
# to start.
class TestE2EMountUnreached < Minitest::Test
  include E2eGuestHelper
  include MountReads

  # Wraps +bind+ as the outer Filesystem, by name: the Bind alone, and an
  # Overlay whose lower Layer it is.
  OUTERS = {
    "Bind" => ->(bind) { bind },
    "Overlay" => ->(bind) { Kobako::Filesystem::Overlay.new(lower: [bind], upper: Kobako::Filesystem::Tmpfs.new) }
  }.freeze

  # A Sandbox with +outer+ wrapping a Bind over +host+ at +/data+, whose
  # +inner+ is a link to +real/+, and a Tmpfs at +at+.
  def through_link(host, outer, at)
    FileUtils.mkdir_p(File.join(host, "real", "x"))
    File.symlink("real", File.join(host, "inner"))
    Kobako::Sandbox.new(wasm_path: REAL_WASM)
                   .mount("/data", outer.call(Kobako::Filesystem::Bind.new(host)), access: :read)
                   .mount(at, Kobako::Filesystem::Tmpfs.new(INNER), access: :read)
  end

  # @behavior MT-041
  def test_a_mount_whose_path_passes_a_link_fails_the_run
    skip "this host makes no links" if Gem.win_platform?
    OUTERS.each do |kind, outer|
      %w[/data/inner /data/inner/x].each do |at|
        TestPaths.host_directory do |host|
          error = assert_raises(Kobako::SandboxError) { through_link(host, outer, at).eval("nil") }
          assert_includes error.message, %("#{at}" stands nowhere: ELOOP),
                          "Sandbox#eval with a mount at #{at}, past a link in a #{kind}, must fail naming it"
        end
      end
    end
  end

  # @behavior MT-051
  def test_a_mount_whose_path_its_parent_lacks_as_a_directory_fails_the_run
    { "/data/x" => "ENOENT", "/data/x/y" => "ENOENT", "/data/f" => "ENOTDIR" }.each do |at, refusal|
      TestPaths.host_directory do |host|
        File.write(File.join(host, "f"), "")
        error = assert_raises(Kobako::SandboxError) { bind_with(host, at => INNER).eval("nil") }
        assert_includes error.message, %("#{at}" stands nowhere: #{refusal}),
                        "Sandbox#eval with a mount at #{at}, which its parent lacks as a directory, must fail naming it"
      end
    end
  end
end
