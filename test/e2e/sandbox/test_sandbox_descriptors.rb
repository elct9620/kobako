# frozen_string_literal: true

require "test_helper"

# E2E — what a Sandbox's default open-file cap leaves the Host App, in a
# child whose host holds 256 descriptors at most.
class TestE2ESandboxDescriptors < Minitest::Test
  include DescriptorChild

  # Each file a script opens beneath a Bind holds one host descriptor.
  OPENS = <<~GUEST
    opened = []
    begin
      300.times { |at| opened << File.open("/b0/d" + at.to_s + "/note.txt") }
    rescue Errno::EMFILE
    end
    opened.size
  GUEST

  CASE = <<~RUBY.freeze
    root = tree("b0", count: 300)
    sandboxed = Kobako::Sandbox.new(wasm_path: WASM).mount("/b0", Kobako::Filesystem::Bind.new(root))
    held = []
    opened = sandboxed.eval(#{OPENS.inspect}).value
    begin
      100.times { held << File.open(File::NULL) }
    rescue Errno::EMFILE
    end
    print [opened, held.size].inspect
  RUBY

  # @behavior S-248
  def test_the_default_open_file_cap_leaves_the_host_app_its_descriptors
    assert_equal "[12, 100]", run_descriptor_child(CASE),
                 "a script under the default open-file cap must open 12 files beneath a Bind, and the Host App " \
                 "100 more after it"
  end
end
