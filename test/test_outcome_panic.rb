# frozen_string_literal: true

require "test_helper"

# Kobako::Outcome::Panic is the wire-shaped failure record carried in
# the OUTCOME_BUFFER's panic branch (SPEC.md Outcome Envelope → Panic).
# Its initialize raises +ArgumentError+ for any wire-illegal input so the
# value object is the single source of truth for field-type invariants;
# the host can rely on a successfully constructed Panic to carry the
# right shapes without re-checking. The decode path uses
# +Kobako::Codec.translate_value_object_error+ to surface these
# +ArgumentError+s as +Codec::InvalidType+, but the rejection itself is
# pinned here at the value-object level.
class TestOutcomePanic < Minitest::Test
  def test_origin_must_be_string
    assert_raises(ArgumentError) do
      Kobako::Outcome::Panic.new(origin: 123, klass: "E", message: "m")
    end
  end

  def test_klass_must_be_string
    assert_raises(ArgumentError) do
      Kobako::Outcome::Panic.new(origin: "sandbox", klass: :sym, message: "m")
    end
  end

  def test_message_must_be_string
    assert_raises(ArgumentError) do
      Kobako::Outcome::Panic.new(origin: "sandbox", klass: "E", message: nil)
    end
  end

  def test_backtrace_must_be_array
    assert_raises(ArgumentError) do
      Kobako::Outcome::Panic.new(origin: "sandbox", klass: "E", message: "m", backtrace: "str")
    end
  end

  def test_backtrace_elements_must_all_be_strings
    assert_raises(ArgumentError) do
      Kobako::Outcome::Panic.new(origin: "sandbox", klass: "E", message: "m", backtrace: ["ok", 42])
    end
  end

  # Belt-and-suspenders: the happy path constructs cleanly with both the
  # required-only form and the full kwargs form. Pins that the validation
  # branches above do not over-reach into legitimate inputs.
  def test_required_only_construction_succeeds
    panic = Kobako::Outcome::Panic.new(origin: "sandbox", klass: "RuntimeError", message: "boom")

    assert_equal "sandbox", panic.origin
    assert_equal "RuntimeError", panic.klass
    assert_equal "boom", panic.message
    assert_equal [], panic.backtrace
    assert_nil panic.details
  end

  def test_full_kwargs_construction_succeeds
    panic = Kobako::Outcome::Panic.new(
      origin: "service",
      klass: "Kobako::ServiceError",
      message: "service failed",
      backtrace: ["a.rb:1", "b.rb:2"],
      details: { "type" => "runtime" }
    )

    assert_equal ["a.rb:1", "b.rb:2"], panic.backtrace
    assert_equal({ "type" => "runtime" }, panic.details)
  end

  def test_origin_constants_match_spec_strings
    assert_equal "sandbox", Kobako::Outcome::Panic::ORIGIN_SANDBOX
    assert_equal "service", Kobako::Outcome::Panic::ORIGIN_SERVICE
  end
end
