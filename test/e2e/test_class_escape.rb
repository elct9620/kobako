# frozen_string_literal: true

require "test_helper"
require "tempfile"

# E2E (Layer 4) — sandbox-escape regression for GHSA-26f3-4cp2-gg6m,
# driven through the real mruby guest (`data/kobako.wasm`).
#
# A class-level method (`File.popen`, `File.read`, `File.new`,
# `Kernel.system`) is owned by the receiver's singleton class, which no
# fixed core-module list can enumerate — so a `Class` or `Module` reaching a
# guest exposed its entire class-level API: host command execution and
# arbitrary file disclosure, while the guest never names a reflection method.
#
# Two paths bring a Class/Module to a guest, closed by two complementary
# guards:
#   * A bound Service that returns a bare Class/Module (the advisory PoC) is
#     refused at the Handle mint point (B-43) — it never crosses.
#   * A Class/Module bound directly as a Service has its class-level methods
#     refused by the owner-based dispatch floor (B-42), which now treats a
#     singleton-class owner as ambient surface.
# `bind` is meant for a Host App's own domain objects, whose ordinary
# instance methods (owner = that class) stay reachable; the high-privilege
# class-level surface of Class / Module / Kernel is what is denied.
class TestE2EClassEscape < Minitest::Test
  include E2eGuestHelper

  SENTINEL = "kobako-escape-sentinel-3f9c"

  # A Service that returns a bare Class / Module as a type tag — the
  # specific, non-default choice that would hand a guest an unwrappable
  # domain class. +File+ is the worst case: +File.popen+ inherits +IO+'s
  # singleton surface.
  class TagService
    def backend_type = File
    def meta = Kernel
  end

  def real_sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)

  # A host file whose contents the guest must never obtain.
  def with_secret_file
    Tempfile.create("kobako-escape") do |f|
      f.write(SENTINEL)
      f.flush
      yield f.path
    end
  end

  # --- B-43: a returned Class / Module never crosses as a Handle ---

  # @behavior T-132
  def test_returned_class_is_refused_at_the_mint_point
    sandbox = real_sandbox
    sandbox.bind("App::Store", TagService.new)
    err = assert_raises(Kobako::ServiceError,
                        "a Service returning a bare Class must be refused, not minted into a Handle") do
      sandbox.eval("App::Store.backend_type").value
    end
    assert_match(/cannot cross as a Capability Handle/, err.message,
                 "the refusal must name the wire-representation rule, not run any class method")
  end

  # @behavior T-132
  def test_returned_module_is_refused_at_the_mint_point
    sandbox = real_sandbox
    sandbox.bind("App::Store", TagService.new)
    err = assert_raises(Kobako::ServiceError,
                        "a Service returning a bare Module must be refused, not minted into a Handle") do
      sandbox.eval("App::Store.meta").value
    end
    assert_match(/cannot cross as a Capability Handle/, err.message,
                 "the refusal must name the wire-representation rule, not run any module method")
  end

  # Because the Class never crosses, the popen chain dies at the return: no
  # host process is spawned and no command output reaches the guest.
  # @behavior T-132
  def test_returned_class_cannot_be_driven_to_host_command_output
    sandbox = real_sandbox
    sandbox.bind("App::Store", TagService.new)
    script = %(App::Store.backend_type.popen("echo #{SENTINEL}").read)
    err = assert_raises(Kobako::ServiceError,
                        "a File.popen chain through a returned Class must be refused before any process runs") do
      sandbox.eval(script).value
    end
    refute_match(/#{SENTINEL}/, err.message,
                 "no command output may reach the guest — the process must never spawn")
  end

  # --- B-42: a directly-bound Class / Module has class-level methods denied ---

  # @behavior T-133
  def test_directly_bound_module_refuses_a_class_level_command
    sandbox = real_sandbox
    sandbox.bind("App::Kernel", Kernel)
    script = %(App::Kernel.system("echo #{SENTINEL}"))
    err = assert_raises(Kobako::ServiceError,
                        "Kernel.system on a directly-bound Module must be refused by the dispatch floor") do
      sandbox.eval(script).value
    end
    refute_match(/#{SENTINEL}/, err.message,
                 "no command output may reach the guest — the process must never spawn")
  end

  # @behavior T-133
  def test_directly_bound_class_refuses_popen
    sandbox = real_sandbox
    sandbox.bind("App::File", File)
    script = %(App::File.popen("echo #{SENTINEL}").read)
    err = assert_raises(Kobako::ServiceError,
                        "File.popen on a directly-bound Class must be refused by the dispatch floor") do
      sandbox.eval(script).value
    end
    refute_match(/#{SENTINEL}/, err.message,
                 "no command output may reach the guest — the process must never spawn")
  end

  # @behavior T-133
  def test_directly_bound_class_refuses_file_read
    with_secret_file do |path|
      sandbox = real_sandbox
      sandbox.bind("App::File", File)
      script = "App::File.read(#{path.inspect})"
      err = assert_raises(Kobako::ServiceError) { sandbox.eval(script).value }
      refute_match(/#{SENTINEL}/, err.message,
                   "File.read on a directly-bound Class must be refused — no host file contents may reach the guest")
    end
  end
end
