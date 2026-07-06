# frozen_string_literal: true

require "test_helper"

# Differential parity — block yield protocol (SPEC.md B-23..B-30,
# E-21, E-22, E-23): the synchronous yield round-trip, break / next /
# lambda semantics, nested dispatch frames, repeated yields, and the
# escape hatches must observe identically once the SDK grows its
# Yielder seam.
class TestParityYield < Parity::Case
  # SPEC.md B-23 / B-24 / B-29 / B-30: yield round-trips and the
  # never-yielding Service.
  def test_yield_round_trip_pending
    skip "pending SDK yield seam (B-23 B-24 B-29 B-30)"
  end

  # SPEC.md B-25 / B-26 / B-27: break / next / lambda-break semantics.
  def test_break_next_semantics_pending
    skip "pending SDK yield seam (B-25 B-26 B-27)"
  end

  # SPEC.md B-28: nested dispatch frames each hold their own block.
  def test_nested_dispatch_pending
    skip "pending SDK yield seam (B-28)"
  end

  # SPEC.md E-21 / E-22 / E-23: block `return`, unrepresentable block
  # values, and the escaped-Yielder refusal.
  def test_yield_escapes_pending
    skip "pending SDK yield seam (E-21 E-22 E-23)"
  end
end
