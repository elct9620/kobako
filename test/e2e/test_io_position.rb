# frozen_string_literal: true

require "test_helper"

# E2E — a stream with no position of its own, and a copy whose descriptor
# another closed, through the real Guest Binary.
class TestE2EIoPosition < Minitest::Test
  include E2eGuestHelper

  CAPTURED = <<~RUBY
    outcome = lambda do |&call|
      call.call
    rescue StandardError => e
      e.class.to_s
    end
    [outcome.call { $stdout.seek(0) }, outcome.call { $stdout.rewind },
     outcome.call { $stdout.pos }, outcome.call { $stderr.pos = 1 }]
  RUBY

  STALE_COPY = <<~RUBY
    file = File.open("/data/note.txt")
    copy = file.dup
    file.close
    [copy.close, copy.closed?]
  RUBY

  def sandbox
    Kobako::Sandbox.new(wasm_path: REAL_WASM)
                   .mount("/data", Kobako::Filesystem::Tmpfs.new("note.txt" => ""), access: :read_write)
  end

  # @behavior IO-045
  def test_a_captured_stream_has_no_position
    assert_equal ["Errno::ESPIPE"] * 4, sandbox.eval(CAPTURED).value,
                 "$stdout#seek, #rewind, #pos, and $stderr#pos= must raise Errno::ESPIPE, as CRuby's do on a pipe"
  end

  # @behavior IO-046
  def test_a_copy_whose_original_closed_closes_quietly
    assert_equal [nil, true], sandbox.eval(STALE_COPY).value,
                 "IO#close on a copy of a File whose original closed must answer nil and leave the copy closed, " \
                 "as CRuby's close does"
  end
end
