# frozen_string_literal: true

require "test_helper"
require "support/in_memory_file_system"
require "support/extension_fixtures"

# E2E (Layer 4) — the #install mechanism through real mruby. A guest `File`
# idiom whose pure methods run in-guest and whose I/O dispatches to a host
# backend (B-55), the backend resolved either fixed or fresh-per-invocation
# (B-56), install refused after the seal (E-51), and I/O failing closed when
# no backend is bound.
#
# The File source and InMemoryFileSystem backend are illustrative fixtures —
# kobako ships no concrete Extension.
class TestE2EInstall < Minitest::Test
  include E2eGuestHelper
  include ExtensionFixtures

  # B-55: the pure method runs in-guest; the I/O method dispatches to the
  # bound backend.
  def test_pure_method_is_local_and_io_dispatches_to_the_backend
    sandbox = install_file(InMemoryFileSystem.new)

    assert_equal "dir/a.txt", sandbox.eval('File.join("dir", File.basename("x/a.txt"))'),
                 "a pure File method must run in-guest through #install (B-55)"
    assert_equal "hello", sandbox.eval('File.write("a.txt", "hello"); File.read("a.txt")'),
                 "a File I/O method must dispatch to the bound backend (B-55)"
  end

  # B-56 fixed: one backend object across invocations, so a write persists.
  def test_fixed_backend_persists_across_invocations
    sandbox = install_file(InMemoryFileSystem.new)

    sandbox.eval('File.write("k", "v1")')
    assert_equal "v1", sandbox.eval('File.read("k")'),
                 "a fixed backend is one object across invocations, so a write persists (B-56)"
  end

  # B-56 callable: a fresh backend each invocation, so a write cannot leak.
  def test_callable_backend_is_isolated_per_invocation
    sandbox = install_file(-> { InMemoryFileSystem.new })

    sandbox.eval('File.write("k", "v1")')
    refute sandbox.eval('File.exist?("k")'),
           "a callable backend is fresh each invocation, so a write cannot leak (B-56)"
  end

  # B-55: File.open eager-slurps once, then serves the buffer in-guest.
  def test_open_eager_slurps_then_serves_the_buffer_locally
    sandbox = install_file(InMemoryFileSystem.new)

    assert_equal "line", sandbox.eval('File.write("log", "line"); File.open("log") { |f| f.read }'),
                 "File.open serves the eager-slurped buffer in-guest (B-55)"
  end

  # B-56: a callable provider that raises surfaces its own error class (not a
  # Kobako error) and leaves the guest unrun; resolution being per-invocation,
  # the next invocation whose provider succeeds runs normally.
  def test_raising_provider_surfaces_its_own_error_then_recovers_next_invocation
    sandbox = install_file(raise_once_provider)

    assert_raises(RuntimeError, "a raising provider must propagate its own error class through #eval (B-56)") do
      sandbox.eval('$stdout.write("ran")')
    end
    assert_equal "", sandbox.stdout,
                 "the guest must not run when provider resolution raises, so no output is produced (B-56)"
    assert_equal "v", sandbox.eval('File.write("k", "v"); File.read("k")'),
                 "per-invocation resolution must let the next invocation run once the provider succeeds (B-56)"
  end

  # E-51: install after the first invocation has sealed registration.
  def test_install_after_first_invocation_raises
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.eval("1")

    err = assert_raises(ArgumentError) { sandbox.install(file_extension(InMemoryFileSystem.new)) }
    assert_match(/after first Sandbox invocation/, err.message)
  end

  # E-52: an unmet depends_on raises at the first invocation — before the
  # guest runs — naming the missing dependency. Walks the real
  # install -> #eval seam (begin_invocation! sealing the Extension
  # registry) that the unit suite pins only by calling seal! in isolation.
  def test_unmet_dependency_raises_before_the_guest_runs_naming_it
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.install(Kobako::Extension.new(name: :File, source: FILE_SOURCE, depends_on: [:Errno]))

    err = assert_raises(ArgumentError) { sandbox.eval('$stdout.write("ran")') }
    assert_match(/:Errno/, err.message,
                 "an unmet depends_on must raise at the first invocation, naming the missing Extension (E-52)")
    assert_equal "", sandbox.stdout,
                 "an unmet dependency must raise before the guest runs, so no guest output is produced (E-52)"
  end

  # B-57: with depends_on satisfied, both Extensions' snippets replay before
  # the guest runs, so the dependent's method body resolves the depended-on
  # Extension's constant at call time. Installed dependent-first to witness
  # that depends_on asserts presence, not replay order.
  def test_satisfied_dependency_composes_so_the_guest_cross_references_at_call_time
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.install(
      Kobako::Extension.new(name: :Status, source: DEPENDENT_SOURCE, depends_on: [:Errno]),
      Kobako::Extension.new(name: :Errno, source: ERRNO_SOURCE)
    )

    assert_equal 2, sandbox.eval("Status.missing_code"),
                 "a satisfied depends_on must let the guest run and resolve the depended-on " \
                 "Extension's constant at call time, independent of install order (B-57)"
  end

  # B-55: with no backend bound, the pure method still runs and the I/O
  # method fails closed as an undefined target.
  def test_without_a_backend_pure_methods_run_and_io_fails_closed
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.install(Kobako::Extension.new(name: :File, source: FILE_SOURCE))

    assert_equal "a/b", sandbox.eval('File.join("a", "b")'),
                 "a pure method runs even with no backend bound (B-55)"
    assert_raises(Kobako::ServiceError,
                  "a privileged method must fail closed as an undefined-target ServiceError " \
                  "when no backend is bound for the idiom (B-55)") do
      sandbox.eval('File.read("x")')
    end
  end

  private

  # A provider that raises on its first resolution and yields a working
  # backend thereafter, so a test can drive the transient-failure-then-
  # recovery path across two invocations.
  def raise_once_provider
    attempts = [0]
    lambda do
      attempts[0] += 1
      raise "overlay unavailable" if attempts[0] == 1

      InMemoryFileSystem.new
    end
  end

  def file_extension(provider)
    Kobako::Extension.new(
      name: :File,
      source: FILE_SOURCE,
      backend: Kobako::Extension::Backend.new(path: "File", provider: provider)
    )
  end

  def install_file(provider)
    Kobako::Sandbox.new(wasm_path: REAL_WASM).install(file_extension(provider))
  end
end
