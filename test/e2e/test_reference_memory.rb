# frozen_string_literal: true

require "test_helper"

# E2E — what a reference names lives on the host, so only the reference
# itself counts against the guest's memory budget, however large the object
# behind it.
class TestE2EReferenceMemory < Minitest::Test
  include E2eGuestHelper

  # @behavior S-171
  def test_objects_held_by_reference_do_not_count_against_the_memory_limit
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM, memory_limit: 2 << 20)
    blob = Struct.new(:bytes)
    sandbox.bind("Probe::Blob", -> { blob.new("x" * (1 << 20)) })

    assert_equal 16, sandbox.eval("Array.new(16) { Probe::Blob.call }.size").value,
                 "16 references to 1 MiB host objects through #eval under a 2 MiB memory_limit " \
                 "must complete, since the objects stay on the host"
  end

  # An answer that crosses by value must land in guest memory, so one larger
  # than the budget is the budget's to refuse rather than the wire's.
  # @behavior S-173
  def test_an_answer_larger_than_the_memory_limit_fails_as_the_memory_limit
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM, memory_limit: 2 << 20)
    sandbox.bind("Probe::Answer", -> { "x" * (4 << 20) })

    assert_raises(Kobako::MemoryLimitError,
                  "a 4 MiB Service answer through #eval under a 2 MiB memory_limit must end the " \
                  "invocation as the budget's own trap") do
      sandbox.eval("Probe::Answer.call")
    end
  end
end
