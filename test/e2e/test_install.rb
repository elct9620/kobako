# frozen_string_literal: true

require "test_helper"
require "support/in_memory_file_system"

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

  FILE_SOURCE = <<~RUBY
    class File < Kobako::Member
      def self.join(*parts)
        parts.join("/")
      end

      def self.basename(path)
        path.split("/").last || ""
      end

      def self.open(path)
        buffer = Buffer.new(read(path))
        return buffer unless block_given?

        begin
          yield buffer
        ensure
          buffer.close
        end
      end

      class Buffer
        def initialize(content)
          @content = content
        end

        def read
          @content
        end

        def close
          nil
        end
      end
    end
  RUBY

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

  # E-51: install after the first invocation has sealed registration.
  def test_install_after_first_invocation_raises
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.eval("1")

    err = assert_raises(ArgumentError) { sandbox.install(file_extension(InMemoryFileSystem.new)) }
    assert_match(/after first Sandbox invocation/, err.message)
  end

  # B-55: with no backend bound, the pure method still runs and the I/O
  # method fails closed as an undefined target.
  def test_without_a_backend_pure_methods_run_and_io_fails_closed
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.install(Kobako::Extension.new(name: :File, source: FILE_SOURCE))

    assert_equal "a/b", sandbox.eval('File.join("a", "b")'),
                 "a pure method runs even with no backend bound (B-55)"
    assert_raises(Kobako::ServiceError) { sandbox.eval('File.read("x")') }
  end

  private

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
