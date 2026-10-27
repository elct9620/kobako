# frozen_string_literal: true

require "test_helper"
require "fileutils"
require "tmpdir"

# E2E — a Mount's block judging a path as a host that folds case or
# Unicode form stores its names, through the real Guest Binary.
class TestE2EMountSpelling < Minitest::Test
  include E2eGuestHelper

  # A block refusing the stored spelling is all that stands between the
  # guest and the file, so only judging that spelling refuses the read.
  # @behavior RT-120
  def test_a_block_judges_a_folded_spelling_as_the_host_stores_it
    Dir.mktmpdir do |host|
      FileUtils.mkdir_p(File.join(host, "private"))
      File.write(File.join(host, "private/secret"), "hidden")
      skip "the host directory matches names exactly" unless File.exist?(File.join(host, "PRIVATE"))

      assert_equal "Errno::EACCES", read_in_upper_case(host),
                   "a file read in another case through a Bind Mount whose block refuses the spelling " \
                   "its folding host stores must be refused"
    end
  end

  private

  # What the guest meets reading +PRIVATE/SECRET+ beneath +host+ mounted at
  # +/data+, with a block refusing everything under +/data/private/+.
  def read_in_upper_case(host)
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.mount("/data", Kobako::Filesystem::Bind.new(host)) { |at, _| !at.start_with?("/data/private/") }
    sandbox.eval('begin; File.read("/data/PRIVATE/SECRET"); rescue => e; e.class.to_s; end').value
  end
end
