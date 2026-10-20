# frozen_string_literal: true

require "test_helper"

# E2E — which Errno a script sees for the Errno a Ruby Filesystem raises,
# through the real Guest Binary's File.
class TestE2ERubyFilesystemErrno < Minitest::Test
  include E2eGuestHelper

  # A Ruby Filesystem whose open raises the Errno it is handed.
  class RaisingFilesystem
    def initialize(error) = @error = error
    def open(_path, _flags) = raise(@error)
    def stat(_path) = File.stat(__FILE__)
  end

  # EPERM once folded into EACCES, so a script told the two apart wrongly.
  # @behavior RF-015
  def test_an_errno_the_guest_names_reaches_the_script_as_itself
    assert_equal "Errno::EPERM", rescued_class(Errno::EPERM),
                 "an open whose Ruby Filesystem raises Errno::EPERM through Sandbox#eval must raise Errno::EPERM " \
                 "in the script"
  end

  # @behavior RF-016
  def test_an_errno_the_guest_has_no_class_for_reaches_the_script_as_eio
    assert_equal "Errno::EIO", rescued_class(Errno::EAGAIN),
                 "an open whose Ruby Filesystem raises Errno::EAGAIN through Sandbox#eval must raise Errno::EIO " \
                 "in the script"
  end

  private

  def rescued_class(error)
    Kobako::Sandbox.new(wasm_path: REAL_WASM)
                   .mount("/data", RaisingFilesystem.new(error))
                   .eval('begin; File.read("/data/note.txt"); rescue SystemCallError => e; e.class.to_s; end')
                   .value
  end
end
