# frozen_string_literal: true

require "test_helper"

# Item #20 — placeholder error rewiring assertions. The cycle 24 placeholder
# `Kobako::HandleTableError < StandardError` and the cycle 14 placeholder
# `Kobako::Sandbox::OutputLimitExceeded < StandardError` are gone; the
# canonical SPEC hierarchy now anchors every kobako-raised error under
# `Kobako::Error` with the three-class taxonomy.
class TestErrorClassHierarchy < Minitest::Test
  def test_three_top_level_classes_descend_from_kobako_error
    assert Kobako::TrapError < Kobako::Error
    assert Kobako::SandboxError < Kobako::Error
    assert Kobako::ServiceError < Kobako::Error
  end

  def test_handle_table_exhausted_chains_under_sandbox_error
    assert Kobako::HandleTableExhausted < Kobako::HandleTableError
    assert Kobako::HandleTableError < Kobako::SandboxError
  end

  def test_service_error_disconnected_chains_under_service_error
    assert Kobako::ServiceError::Disconnected < Kobako::ServiceError
  end

  def test_sandbox_output_limit_exceeded_placeholder_is_gone
    # Cycle 14 left `Kobako::Sandbox::OutputLimitExceeded < StandardError`
    # as a placeholder; SPEC B-04 specifies truncate-with-marker, not
    # raise. The placeholder must no longer exist.
    refute defined?(Kobako::Sandbox::OutputLimitExceeded),
           "Kobako::Sandbox::OutputLimitExceeded must be removed (SPEC B-04 truncates)"
  end
end
