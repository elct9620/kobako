# frozen_string_literal: true

require "test_helper"

# E2E (Layer 4) — capability-gem FFI exception-safety (SPEC.md B-51).
#
# A capability gem (here the kobako-io write surface, B-04) coerces each
# argument through a guest-supplied `to_s` / `inspect` inside its Rust
# frame. When that guest method raises, the raise must propagate as an
# ordinary guest exception attributed as Kobako::SandboxError (E-04) — it
# must never long-jump across the gem's host-language boundary and surface
# as a Kobako::TrapError (E-01) that retires the Sandbox. A regression
# reintroducing the long-jump surfaces here as a TrapError escaping the
# Kobako::SandboxError expectation.
class TestE2ECapabilityExceptionSafety < Minitest::Test
  include E2eGuestHelper

  RAISING_TO_S_SCRIPT = <<~RUBY
    class Boom
      def to_s
        raise "boom from to_s"
      end
    end
    $stdout.puts(Boom.new)
  RUBY

  # `$stdout.puts` coerces its argument via `to_s`; a raising `to_s` is a
  # guest application error, attributed as Kobako::SandboxError per E-04.
  def test_puts_attributes_a_raising_to_s_as_sandbox_error_not_trap
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)

    err = assert_raises(Kobako::SandboxError) do
      sandbox.eval(RAISING_TO_S_SCRIPT)
    end

    assert_includes err.message, "boom from to_s",
                    "a raising to_s during $stdout.puts coercion must surface through #eval as " \
                    "Kobako::SandboxError carrying the guest exception message, not a TrapError"
  end

  RAISING_INSPECT_SCRIPT = <<~RUBY
    class Boom
      def inspect
        raise "boom from inspect"
      end
    end
    p Boom.new
  RUBY

  # `p` coerces its argument via `inspect` — the second funcall coercion
  # path on the write surface, attributed identically per E-04.
  def test_p_attributes_a_raising_inspect_as_sandbox_error_not_trap
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)

    err = assert_raises(Kobako::SandboxError) do
      sandbox.eval(RAISING_INSPECT_SCRIPT)
    end

    assert_includes err.message, "boom from inspect",
                    "a raising inspect during `p` coercion must surface through #eval as " \
                    "Kobako::SandboxError carrying the guest exception message, not a TrapError"
  end

  # The Sandbox stays usable after a coercion-raised guest error: a
  # SandboxError retires the guest instance normally (unlike a TrapError),
  # so a fresh invocation on the same Sandbox runs cleanly.
  def test_sandbox_recovers_after_a_coercion_raise
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)

    assert_raises(Kobako::SandboxError) { sandbox.eval(RAISING_TO_S_SCRIPT) }

    assert_equal 3, sandbox.eval("1 + 2"),
                 "a guest coercion raise must leave the Sandbox usable for the next #eval"
  end
end
