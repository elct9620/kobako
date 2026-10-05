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
end
