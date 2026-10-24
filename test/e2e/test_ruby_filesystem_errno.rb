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

  # A Ruby Filesystem whose stat names no identity.
  class NamelessFilesystem
    Stat = Data.define(:directory?, :size)

    def open(path, _flags) = raise(Errno::ENOENT, path)
    def stat(path) = Stat.new(path.empty?, 0)
  end

  # A stat must answer +ino+, zero being how it names no identity, so one
  # lacking the method fails the call.
  # @behavior RF-017
  def test_a_ruby_filesystems_stat_names_an_identity
    assert_equal "Errno::EIO", read_class(NamelessFilesystem.new),
                 "a read through Sandbox#eval of a Ruby Filesystem whose stat answers no ino must raise " \
                 "Errno::EIO in the script"
  end

  private

  def rescued_class(error) = read_class(RaisingFilesystem.new(error))

  def read_class(filesystem)
    Kobako::Sandbox.new(wasm_path: REAL_WASM)
                   .mount("/data", filesystem)
                   .eval('begin; File.read("/data/note.txt"); rescue SystemCallError => e; e.class.to_s; end')
                   .value
  end
end
