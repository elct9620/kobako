# frozen_string_literal: true

require "test_helper"
require "fileutils"
require "tmpdir"

# E2E — what an Overlay of Binds counts against the open-file cap, through
# the real Guest Binary. A directory each Bind Layer holds keeps a host
# directory open for each, yet the cap counts the guest's descriptor alone,
# as Linux's RLIMIT_NOFILE does; the three standard descriptors and the root
# preopen leave a cap of 16 room for twelve.
class TestE2EOverlayDescriptors < Minitest::Test
  include E2eGuestHelper

  def setup
    @hosts = Dir.mktmpdir("kobako-overlay-descriptors")
  end

  def teardown
    TestPaths.remove_host_directory(@hosts)
  end

  # A Sandbox under +open_file_limit+ mounting at /o an Overlay of +layers+
  # Binds, each holding d/note.txt, the topmost twenty files beside d.
  def stacked(layers, open_file_limit:)
    binds = Array.new(layers) do |at|
      root = File.join(@hosts, at.to_s)
      FileUtils.mkdir_p(File.join(root, "d"))
      File.write(File.join(root, "d", "note.txt"), "hello")
      20.times { |file| File.write(File.join(root, "f#{file}"), "x") } if at.zero?
      Kobako::Filesystem::Bind.new(root)
    end
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM, open_file_limit: open_file_limit)
    sandbox.mount("/o", Kobako::Filesystem::Overlay.new(lower: binds))
  end

  # What +script+ answers, or the class of what it raised.
  def answer(sandbox, script)
    sandbox.eval(["begin", script, "rescue SystemCallError => e; e.class.to_s; end"].join("\n")).value
  end

  # A Dir reads its listing as it opens, so a File holds the directory.
  OPENS_PAST_A_HELD_DIRECTORY = <<~RUBY
    held = File.open("/o/d")
    opened = []
    begin
      20.times { |file| opened << File.open("/o/f" + file.to_s) }
    rescue Errno::EMFILE
    end
    opened.size
  RUBY

  # @behavior RT-152
  def test_an_overlay_directory_takes_one_descriptor_however_many_bind_layers_hold_it
    assert_equal 11, answer(stacked(8, open_file_limit: 16), OPENS_PAST_A_HELD_DIRECTORY),
                 "File.open past a directory eight Bind Layers of an Overlay hold, under an open-file cap of 16, " \
                 "must open eleven files"
  end

  # @behavior RT-152
  def test_an_overlay_directory_lists_however_many_bind_layers_hold_it
    assert_equal ["note.txt"], answer(stacked(13, open_file_limit: 16), 'Dir.children("/o/d")'),
                 "Dir.children of a directory thirteen Bind Layers of an Overlay hold, under an open-file cap of 16, " \
                 "must list it"
  end
end
