# frozen_string_literal: true

require "test_helper"
require "delegate"

# E2E (Layer 4) — sandbox-escape regression for GHSA-5jxx-2336-22p8,
# driven through the real mruby guest (`data/kobako.wasm`).
#
# A transparent forwarder (`SimpleDelegator` / `DelegateClass` / `WeakRef` /
# `Tempfile`) exposes a public `method_missing` whose owner is `Delegator`,
# outside any core-module list the dispatch floor reads as ambient — so the
# guest could call it explicitly, hand it `:system`, and Ruby's forwarder
# bound and called the private `Kernel#system` in the host process.
#
# Two guards close it, one per path a forwarder crosses on:
#   * A forwarder answered by a Service or passed as a #run argument is
#     refused at the Handle mint point — it never crosses as a reference.
#   * A forwarder bound directly as a Service has its dynamic-dispatch hook
#     refused by the owner-based floor, which now denies an explicit
#     `method_missing` whatever its owner.
# `bind` is meant for a Host App's own domain objects, whose ordinary
# instance methods (owner = that class) stay reachable; a wrapper whose whole
# surface is arbitrary forwarding is what is denied.
class TestE2EDelegatorEscape < Minitest::Test
  include E2eGuestHelper

  def real_sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)

  # A Service that answers a transparent forwarder as an ordinary value.
  class WrapService
    def wrapped = SimpleDelegator.new(Object.new)
  end

  # A marker path the guest must never cause the host to write: its presence
  # is proof a host command ran.
  def with_marker_path
    dir = Dir.mktmpdir("kobako-delegator-escape")
    yield File.join(dir, "marker")
  ensure
    FileUtils.remove_entry(dir) if dir
  end

  # --- a forwarder answered or passed never crosses as a reference (T-206) ---

  # @behavior T-206
  def test_returned_forwarder_is_refused_at_the_mint_point
    sandbox = real_sandbox
    sandbox.bind("App::Store", WrapService.new)
    err = assert_raises(Kobako::ServiceError,
                        "a Service answering a transparent forwarder must be refused, not minted into a Handle") do
      sandbox.eval("App::Store.wrapped").value
    end
    assert_match(/cannot cross as a Capability Handle/, err.message,
                 "the refusal must name the wire-representation rule, not run any forwarded method")
  end

  # @behavior T-206
  def test_forwarder_run_argument_is_refused_at_the_mint_point
    sandbox = real_sandbox
    sandbox.preload(code: 'App = ->(t) { t.__send__(:method_missing, :method_missing, :system, "x") }', name: "App")
    err = assert_raises(Kobako::SandboxError) { sandbox.run(:App, SimpleDelegator.new(Object.new)).value }
    assert_match(/cannot cross as a Capability Handle/, err.message,
                 "a forwarder passed as a #run argument must be refused before the guest runs")
  end

  # Because the forwarder never crosses, its dynamic-dispatch hook cannot be
  # reached to run a host command: the chain dies at the answer.
  # @behavior T-206
  def test_returned_forwarder_cannot_be_driven_to_host_command
    with_marker_path do |marker|
      sandbox = real_sandbox
      sandbox.bind("App::Store", WrapService.new)
      script = %(App::Store.wrapped.method_missing(:method_missing, :system, "/bin/sh", "-c", "touch #{marker}"))
      assert_raises(Kobako::ServiceError) { sandbox.eval(script).value }
      refute_path_exists(marker,
                         "a method_missing chain through a returned forwarder must run no host command")
    end
  end

  # --- a forwarder bound directly has its dispatch hook denied (T-207) ---

  # @behavior T-207
  def test_directly_bound_forwarder_refuses_its_dispatch_hook
    with_marker_path do |marker|
      sandbox = real_sandbox
      sandbox.bind("App::Wrap", SimpleDelegator.new(Object.new))
      script = %(App::Wrap.method_missing(:method_missing, :system, "/bin/sh", "-c", "touch #{marker}"))
      assert_raises(Kobako::ServiceError) { sandbox.eval(script).value }
      refute_path_exists(marker,
                         "an explicit method_missing on a directly-bound forwarder must run no host command")
    end
  end
end
