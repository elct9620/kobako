# frozen_string_literal: true

# Tests for build_config/wasi.rb and tasks/mruby.rake.
#
# Two-tier pattern (mirrors test_vendor_task.rb's design):
#
#   * Fast tier — always runs. Loads `build_config/wasi.rb` with a stubbed
#     MRuby::Build shim and asserts that the five customisation rules from
#     REFERENCE.md Ch.5 §mruby 客製化五條 are enforced: pinned wasi-sdk
#     toolchain, three setjmp/longjmp flags, MRB_WORDBOX_NO_INLINE_FLOAT
#     and MRB_INT32 defines, and the mrbgem allowlist.
#
#   * Real tier — gated by `KOBAKO_E2E_BUILD=1`. Actually invokes
#     `rake mruby:build` against a real vendored toolchain and asserts the
#     resulting libmruby.a is a wasm32 archive. Skips with an informative
#     message when the env flag is unset, or when the vendored toolchain is
#     not pre-fetched (mruby's full build can take minutes and pulls
#     hundreds of MiB; we don't auto-trigger it from CI).
#
# Intentionally does NOT require "test_helper" — that file loads the native
# extension which doesn't exist in clean checkouts.

require "minitest/autorun"
require "fileutils"
require "open3"
require "rbconfig"
require "tmpdir"

class TestMrubyBuildConfig < Minitest::Test
  PROJECT_ROOT  = File.expand_path("..", __dir__)
  CONFIG_PATH   = File.join(PROJECT_ROOT, "build_config", "wasi.rb")

  # ---- Fast tier: stubbed-MRuby evaluation of build_config/wasi.rb -------

  # Minimal MRuby::Build stand-in. The real DSL is rich, but the surface
  # exercised by build_config/wasi.rb is small and stable: a few mutable
  # `<<`-able lists per command (cc/cxx/linker/archiver) plus a `gem core:`
  # call and a `toolchain` call.
  class FakeFlagList
    def initialize
      @items = []
    end

    def <<(other)
      if other.is_a?(Array)
        @items.concat(other)
      else
        @items << other
      end
      self
    end

    def to_a
      @items.dup
    end

    def include?(value)
      @items.include?(value)
    end

    # Match `[flag1, flag2]` as a *contiguous subsequence* in the flag list,
    # since clang flags like `-mllvm -wasm-enable-sjlj` are positional pairs.
    def include_sequence?(seq)
      seq = Array(seq)
      return true if seq.empty?

      (0..(@items.length - seq.length)).any? do |i|
        @items[i, seq.length] == seq
      end
    end
  end

  class FakeCommand
    attr_accessor :command
    attr_reader :flags, :defines, :libraries

    def initialize
      @flags = FakeFlagList.new
      @defines = FakeFlagList.new
      @libraries = FakeFlagList.new
    end
  end

  class FakeBuild
    attr_reader :name, :cc, :cxx, :linker, :archiver, :asm, :gems, :toolchain_name

    def initialize(name)
      @name = name
      @cc = FakeCommand.new
      @cxx = FakeCommand.new
      @linker = FakeCommand.new
      @archiver = FakeCommand.new
      @asm = FakeCommand.new
      @gems = []
      @toolchain_name = nil
    end

    def toolchain(name)
      @toolchain_name = name
    end

    def gem(spec)
      @gems << spec
    end

    def gembox(_name); end
    def disable_presym; end
  end

  # Capture the build object produced when build_config/wasi.rb is loaded.
  def load_config
    captured = nil
    fake_build_class = Class.new do
      define_singleton_method(:new) do |name, &block|
        b = FakeBuild.new(name)
        block&.call(b)
        captured = b
        b
      end
    end

    # Fresh anonymous MRuby module per call so tests don't leak state.
    mruby_mod = Module.new
    mruby_mod.const_set(:Build, fake_build_class)
    Object.const_set(:MRuby, mruby_mod) unless Object.const_defined?(:MRuby)
    # If MRuby is already defined (e.g. from a previous test run in the same
    # process), swap its Build constant for ours.
    Object.const_get(:MRuby).send(:remove_const, :Build) if Object.const_get(:MRuby).const_defined?(:Build)
    Object.const_get(:MRuby).const_set(:Build, fake_build_class)

    load CONFIG_PATH
    captured
  end

  def test_target_name_is_wasi
    build = load_config
    assert_equal "wasi", build.name,
                 "MRuby::Build name must be 'wasi' so libmruby.a lands in build/wasi/"
  end

  def test_uses_clang_toolchain
    build = load_config
    assert_equal :clang, build.toolchain_name
  end

  def test_cc_points_at_vendored_wasi_sdk
    build = load_config
    assert_match %r{vendor/wasi-sdk/bin/clang\z}, build.cc.command,
                 "CC must resolve to vendored wasi-sdk clang (rule #2)"
    assert_match %r{vendor/wasi-sdk/bin/clang\z}, build.linker.command,
                 "LD must resolve to vendored wasi-sdk clang (rule #2)"
    assert_match %r{vendor/wasi-sdk/bin/llvm-ar\z}, build.archiver.command,
                 "AR must resolve to vendored wasi-sdk llvm-ar (rule #2)"
  end

  def test_cross_compile_target_is_wasm32_wasi
    build = load_config
    assert build.cc.flags.include?("--target=wasm32-wasi"),
           "cc flags must pin --target=wasm32-wasi"
    assert build.linker.flags.include?("--target=wasm32-wasi"),
           "linker flags must pin --target=wasm32-wasi"
  end

  def test_setjmp_longjmp_three_flag_set_on_compile_and_link
    build = load_config

    # The two `-mllvm` paired flags must appear as adjacent pairs.
    assert build.cc.flags.include_sequence?(["-mllvm", "-wasm-enable-sjlj"]),
           "cc flags must include `-mllvm -wasm-enable-sjlj` (REFERENCE Ch.5)"
    assert build.cc.flags.include_sequence?(["-mllvm", "-wasm-use-legacy-eh=false"]),
           "cc flags must include `-mllvm -wasm-use-legacy-eh=false` (REFERENCE Ch.5)"
    assert build.linker.flags.include_sequence?(["-mllvm", "-wasm-enable-sjlj"]),
           "linker flags must also include `-mllvm -wasm-enable-sjlj` (3-flag set is compile+link)"
    assert build.linker.flags.include_sequence?(["-mllvm", "-wasm-use-legacy-eh=false"]),
           "linker flags must also include `-mllvm -wasm-use-legacy-eh=false`"

    # libsetjmp must be linked against. We accept either `-lsetjmp` directly
    # or `linker.libraries << "setjmp"` (mruby's DSL expands the latter to
    # the former when emitting the link command).
    has_lsetjmp = build.linker.flags.include?("-lsetjmp") ||
                  build.linker.libraries.include?("setjmp")
    assert has_lsetjmp, "linker must pull in libsetjmp (`-lsetjmp` from wasi-libc)"
  end

  def test_mrb_wordbox_no_inline_float_define
    build = load_config
    assert build.cc.defines.include?("MRB_WORDBOX_NO_INLINE_FLOAT"),
           "MRB_WORDBOX_NO_INLINE_FLOAT must be defined (rule #4 — pins mrb_value layout)"
  end

  def test_mrb_int32_define
    build = load_config
    assert build.cc.defines.include?("MRB_INT32"),
           "MRB_INT32 must be defined (REFERENCE Ch.5 整數寬度)"
  end

  def test_does_not_set_vm_switch_dispatch
    build = load_config
    refute build.cc.defines.include?("MRB_USE_VM_SWITCH_DISPATCH"),
           "rule #5: MRB_USE_VM_SWITCH_DISPATCH must NOT be set; use mruby default"
  end

  def test_mrbgem_allowlist_includes_required_core_gems
    build = load_config
    gem_names = build.gems.map { |g| g[:core] || g["core"] }.compact

    # REFERENCE Ch.5 explicitly names these as required core gems.
    %w[mruby-string-ext mruby-array-ext mruby-hash-ext].each do |required|
      assert_includes gem_names, required,
                      "allowlist must include core extension gem #{required.inspect}"
    end
  end

  def test_mrbgem_allowlist_excludes_io_and_network_gems
    build = load_config
    gem_names = build.gems.map { |g| g[:core] || g["core"] }.compact

    # Allowlist == strict opt-in; REFERENCE Ch.5 explicitly forbids I/O,
    # network, sleep, random-seed gems.
    forbidden = %w[
      mruby-io
      mruby-socket
      mruby-sleep
      mruby-random
      mruby-process
      mruby-dir
      mruby-time
    ]

    forbidden.each do |gem|
      refute_includes gem_names, gem,
                      "allowlist must NOT include #{gem.inspect} (I/O / network / sleep / random)"
    end
  end
end

# ---- Real tier: gated end-to-end mruby build -----------------------------
#
# Set `KOBAKO_E2E_BUILD=1` to enable. Also requires the vendored toolchain
# (vendor/wasi-sdk + vendor/mruby) to already exist; we skip rather than
# auto-fetch because the wasi-sdk download is hundreds of MiB.
class TestMrubyBuildE2E < Minitest::Test
  PROJECT_ROOT = File.expand_path("..", __dir__)

  def test_rake_mruby_build_produces_wasm32_libmruby
    skip "set KOBAKO_E2E_BUILD=1 to run the real mruby build" unless ENV["KOBAKO_E2E_BUILD"] == "1"

    wasi_sdk = File.join(PROJECT_ROOT, "vendor", "wasi-sdk", "bin", "clang")
    mruby_dir = File.join(PROJECT_ROOT, "vendor", "mruby", "Rakefile")
    unless File.exist?(wasi_sdk) && File.exist?(mruby_dir)
      skip "vendored toolchain missing (run `rake vendor:setup` first to enable this test)"
    end

    libmruby = File.join(PROJECT_ROOT, "vendor", "mruby", "build", "wasi", "lib", "libmruby.a")

    out, status = Open3.capture2e(RbConfig.ruby, "-S", "rake", "mruby:build", chdir: PROJECT_ROOT)
    assert status.success?, "rake mruby:build failed:\n#{out}"
    assert File.exist?(libmruby), "libmruby.a missing at #{libmruby}"

    # Inspect the archive to confirm it contains wasm32 object files. Two
    # acceptable signals: `file(1)` reports "WebAssembly", or `llvm-ar t`
    # lists `.o` members and `llvm-objdump` recognises wasm format.
    file_out, file_status = Open3.capture2e("file", libmruby)
    assert file_status.success?, "could not run `file` against libmruby.a:\n#{file_out}"
    assert_match(/(WebAssembly|wasm|current ar archive)/i, file_out,
                 "libmruby.a should look like a wasm-targeted ar archive: #{file_out}")

    llvm_ar = File.join(PROJECT_ROOT, "vendor", "wasi-sdk", "bin", "llvm-ar")
    if File.exist?(llvm_ar)
      ar_out, ar_status = Open3.capture2e(llvm_ar, "t", libmruby)
      assert ar_status.success?, "llvm-ar t failed:\n#{ar_out}"
      assert_match(/\.o$/, ar_out, "libmruby.a should contain at least one .o member")
    end

    # Idempotency: a second invocation must be a no-op (short-circuits on
    # the libmruby.a sentinel).
    out2, status2 = Open3.capture2e(RbConfig.ruby, "-S", "rake", "mruby:build", chdir: PROJECT_ROOT)
    assert status2.success?, "second rake mruby:build failed:\n#{out2}"
    assert_match(/already present|skipping/i, out2,
                 "second invocation should short-circuit, but output was:\n#{out2}")
  end
end
