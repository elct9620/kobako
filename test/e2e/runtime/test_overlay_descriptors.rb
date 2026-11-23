# frozen_string_literal: true

require "test_helper"
require "fileutils"
require "tmpdir"

# E2E — what an Overlay of Binds counts against the open-file cap, through
# the real Guest Binary. A directory each Bind Layer holds keeps a host
# directory open for each, so it counts one for each; the three standard
# descriptors and the root preopen leave a cap of 16 room for twelve.
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
  def test_an_overlay_directory_takes_a_descriptor_for_each_bind_layer
    assert_equal 4, answer(stacked(8, open_file_limit: 16), OPENS_PAST_A_HELD_DIRECTORY),
                 "File.open past a directory eight Bind Layers of an Overlay hold, under an open-file cap of 16, " \
                 "must open four files"
  end

  # @behavior RT-152
  def test_an_overlay_directory_lists_only_with_room_for_each_bind_layer
    answers = [16, 17].map { |limit| answer(stacked(13, open_file_limit: limit), 'Dir.children("/o/d")') }
    assert_equal ["Errno::EMFILE", ["note.txt"]], answers,
                 "Dir.children of a directory thirteen Bind Layers of an Overlay hold must raise Errno::EMFILE " \
                 "under an open-file cap of 16 and list it under 17"
  end

  # @behavior RT-414
  def test_an_overlay_root_lists_however_many_binds_it_stacks
    names = answer(stacked(13, open_file_limit: 16), 'Dir.children("/o").sort')
    assert_equal ["d", *Array.new(20) { |file| "f#{file}" }].sort, names,
                 "Dir.children of the root of an Overlay of thirteen Binds, under an open-file cap of 16, must list it"
  end

  # @behavior FL-107
  def test_a_glob_through_a_directory_with_no_room_raises
    assert_equal "Errno::EMFILE", answer(stacked(13, open_file_limit: 16), 'Dir.glob("/o/d/*")'),
                 "Dir.glob through a directory thirteen Bind Layers of an Overlay hold, under an open-file cap " \
                 "of 16, must raise Errno::EMFILE"
  end
end
