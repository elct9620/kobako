# frozen_string_literal: true

require "test_helper"

# E2E — the +open_file_limit+ cap through the real Guest Binary: the files a
# script may hold open at once, the standard descriptors and each Mount's
# included, and the Errno::EMFILE an open past it raises.
class TestE2EOpenFileLimit < Minitest::Test
  include E2eGuestHelper

  # Hold every file opened until one is refused, answering how many opened.
  OPEN_UNTIL_REFUSED = <<~RUBY
    held = []
    begin
      10.times { held << File.open("/data/note.txt") }
      [held.size, :never_refused]
    rescue Errno::EMFILE
      [held.size, :went_on]
    end
  RUBY

  # Three standard descriptors and the Mount leave room for +room+ files.
  def sandbox(room: 2)
    Kobako::Sandbox.new(wasm_path: REAL_WASM, open_file_limit: 4 + room)
                   .mount("/data", Kobako::Filesystem::Memory.new("note.txt" => "hello"))
  end

  # @behavior S-201
  def test_every_open_descriptor_counts_toward_the_cap
    assert_equal [1, :went_on], sandbox(room: 1).eval(OPEN_UNTIL_REFUSED).value,
                 "open_file_limit one above the standard descriptors and a Mount through Sandbox#eval must " \
                 "let exactly one file open"
  end

  # @behavior S-200
  def test_an_open_past_the_cap_raises_emfile_and_the_script_goes_on
    assert_equal [2, :went_on], sandbox.eval(OPEN_UNTIL_REFUSED).value,
                 "an open past open_file_limit through Sandbox#eval must raise Errno::EMFILE the script rescues"
  end

  # @behavior S-202
  def test_the_cap_is_per_invocation
    sandbox = self.sandbox
    sandbox.eval(OPEN_UNTIL_REFUSED)

    assert_equal [2, :went_on], sandbox.eval(OPEN_UNTIL_REFUSED).value,
                 "a later invocation through Sandbox#eval must open as many files as the first did"
  end
end
