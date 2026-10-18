# frozen_string_literal: true

require "test_helper"

# E2E — the +limit:+ a Host App gives a Memory or a Temporary, reaching the
# guest's File through the real Guest Binary as Errno::ENOSPC.
class TestE2EMountLimit < Minitest::Test
  include E2eGuestHelper

  # Write five bytes beneath +/data+, answering +:refused+ when the
  # Filesystem has no room, then what +note.txt+ reads if it exists.
  SCRIPT = <<~RUBY
    begin
      File.open("/data/scratch", "w") { |file| file.write("five!") }
      [:written]
    rescue Errno::ENOSPC
      [:refused, File.exist?("/data/note.txt") ? File.read("/data/note.txt") : nil]
    end
  RUBY

  def write_beneath(filesystem)
    Kobako::Sandbox.new(wasm_path: REAL_WASM).mount("/data", filesystem, access: :read_write).eval(SCRIPT).value
  end

  # The seeded file rides as a bare keyword beside +limit:+, so both must
  # still reach the Memory.
  # @behavior MT-013
  def test_a_memorys_limit_given_in_ruby_bounds_what_the_guest_writes
    assert_equal [:refused, "hello"], write_beneath(Kobako::Filesystem::Memory.new("note.txt" => "hello", limit: 8)),
                 "a write past a limit given through Memory.new must raise Errno::ENOSPC in the guest " \
                 "and leave the seeded files"
  end

  # @behavior MT-014
  def test_a_temporarys_limit_given_in_ruby_bounds_what_the_guest_writes
    assert_equal [:refused, nil], write_beneath(Kobako::Filesystem::Temporary.new(limit: 4)),
                 "a write past a limit given through Temporary.new must raise Errno::ENOSPC in the guest"
  end
end
