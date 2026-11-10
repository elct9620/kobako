# frozen_string_literal: true

require "test_helper"

# E2E — what a script's calls about a file's position and end answer where
# it has none, through the real Guest Binary: a directory opened as a File,
# and a file opened only to write.
class TestE2EFilePosition < Minitest::Test
  include E2eGuestHelper

  OUTCOME = <<~RUBY
    outcome = lambda do |&call|
      call.call
    rescue StandardError => e
      [e.class.to_s, e.message]
    end
  RUBY

  DIRECTORY = <<~RUBY.freeze
    #{OUTCOME}
    dir = File.open("/data")
    [outcome.call { dir.seek(0) }, outcome.call { dir.rewind },
     outcome.call { dir.pos }, outcome.call { dir.pos = 3 }].map(&:first)
  RUBY

  WRITE_ONLY = <<~RUBY.freeze
    #{OUTCOME}
    outcome.call { File.open("/data/note.txt", "w").eof? }
  RUBY

  def sandbox
    Kobako::Sandbox.new(wasm_path: REAL_WASM)
                   .mount("/data", Kobako::Filesystem::Tmpfs.new("note.txt" => "hello"), access: :read_write)
  end

  # @behavior FL-077
  def test_a_directory_opened_as_a_file_has_no_position
    assert_equal ["Errno::EISDIR"] * 4, sandbox.eval(DIRECTORY).value,
                 "File#seek, #rewind, #pos, and #pos= on a directory opened as a File must raise Errno::EISDIR, " \
                 "since WASI keeps no position for a directory where CRuby's answers one"
  end

  # @behavior FL-078
  def test_a_file_opened_only_to_write_refuses_eof
    assert_equal ["IOError", "not opened for reading"], sandbox.eval(WRITE_ONLY).value,
                 "File#eof? on a file opened \"w\" must raise IOError \"not opened for reading\", as CRuby's does"
  end
end
