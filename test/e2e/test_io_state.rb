# frozen_string_literal: true

require "test_helper"

# E2E — what a kobako-io stream carries through real mruby: the descriptor
# a copy keeps after its original closes, and the bookkeeping a script
# cannot see. Both need a file of the guest's own, so each runs over a
# Mount.
class TestE2EIoState < Minitest::Test
  include E2eGuestHelper

  # A guest that writes through a copy of a file it already closed — the
  # copy still carries the released descriptor.
  STALE_COPY_SCRIPT = <<~RUBY
    file = File.open("/data/note.txt", "w")
    copy = file.dup
    file.close
    copy.write("escape")
  RUBY

  def sandbox
    Kobako::Sandbox.new(wasm_path: REAL_WASM)
                   .mount("/data", Kobako::Filesystem::Memory.new("note.txt" => ""), access: :read_write)
  end

  # @behavior IO-003
  # The fd allowlist is enforced at the write syscall, not only at
  # construction: a copy keeps the descriptor its original released, so a
  # write through it must be refused rather than reaching write(2) on a
  # number the guest no longer holds. Without the use-point check the
  # construction-time allowlist would be a courtesy.
  def test_io_write_rejects_a_descriptor_its_original_closed
    err = assert_raises(Kobako::SandboxError) { sandbox.eval(STALE_COPY_SCRIPT) }

    assert_includes err.message, "kobako IO writes only to fd",
                    "a write through a copy of a closed File through Sandbox#eval must be refused at IO#write, " \
                    "not forwarded to write(2)"
  end

  # @behavior IO-037
  # A stream's descriptor, path, and sync flag are kobako's bookkeeping;
  # CRuby shows none of them, so a script listing them must see nothing.
  def test_a_stream_shows_no_instance_variable
    lists = sandbox.eval(<<~RUBY).value
      $stdout.sync = false
      [$stdout.instance_variables, File.open("/data/note.txt").instance_variables]
    RUBY

    assert_equal [[], []], lists,
                 "instance_variables on $stdout and an opened File through Sandbox#eval must be empty"
  end
end
