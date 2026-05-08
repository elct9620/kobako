# frozen_string_literal: true

# Fast-tier test for item #11 (Guest Binary build pipeline).
#
# Asserts the *wiring* of Stage C without actually invoking `cargo build`:
#
#   * `wasm:guest` task is defined and chains the prerequisites that walk
#     the full Stage A → B → C pipeline (vendor:setup → mruby:build → this).
#   * `wasm:guest:clean` companion task is defined.
#   * The wasm crate's `build.rs` exists and references the env-var-driven
#     mruby static-lib linkage contract documented in tasks/wasm.rake.
#   * Both files parse cleanly: tasks/wasm.rake as Ruby (RubyVM compile),
#     build.rs as Rust (`cargo check` on the wasm crate; this also validates
#     the build.rs is honored by cargo without crashing the whole build).
#
# The actual cross-compile is exercised by test/test_wasm_guest_build.rb,
# which is gated on KOBAKO_E2E_BUILD=1 because it needs the vendored
# wasi-sdk + libmruby.a (heavy).
#
# Intentionally does NOT require "test_helper" — like the other build-time
# tests in this directory, it inspects build artifacts and does not load
# the native extension.

require "minitest/autorun"
require "rake"

class TestWasmGuestPipeline < Minitest::Test
  PROJECT_ROOT = File.expand_path("..", __dir__)
  WASM_RAKE    = File.join(PROJECT_ROOT, "tasks", "wasm.rake")
  BUILD_RS     = File.join(PROJECT_ROOT, "wasm", "kobako-wasm", "build.rs")

  # Load the rake tasks into a fresh Rake::Application per test class so
  # task definitions don't leak across files. Each test method works on the
  # same loaded app — sufficient for read-only assertions.
  def self.application
    @application ||= begin
      app = Rake::Application.new
      Rake.application = app
      Rake::TaskManager.record_task_metadata = true
      load File.join(PROJECT_ROOT, "tasks", "vendor.rake")
      load File.join(PROJECT_ROOT, "tasks", "mruby.rake")
      load WASM_RAKE
      app
    end
  end

  def app
    self.class.application
  end

  # ---- Task wiring -------------------------------------------------------

  def test_wasm_guest_task_is_defined
    refute_nil app.lookup("wasm:guest"),
               "tasks/wasm.rake must define the `wasm:guest` task (Stage C)"
  end

  def test_wasm_guest_clean_task_is_defined
    refute_nil app.lookup("wasm:guest:clean"),
               "tasks/wasm.rake must define the companion `wasm:guest:clean` task"
  end

  def test_wasm_guest_chains_full_pipeline
    task = app.lookup("wasm:guest")
    refute_nil task
    prereqs = task.prerequisites
    assert_includes prereqs, "vendor:setup",
                    "wasm:guest must depend on vendor:setup (Stage A)"
    assert_includes prereqs, "mruby:build",
                    "wasm:guest must depend on mruby:build (Stage B); saw: #{prereqs.inspect}"
  end

  # ---- build.rs presence + content -------------------------------------

  def test_build_rs_exists
    assert File.file?(BUILD_RS),
           "wasm/kobako-wasm/build.rs must exist for Stage C to wire libmruby.a"
  end

  def test_build_rs_emits_link_search_for_mruby_lib_dir
    contents = File.read(BUILD_RS)
    # The rake driver exports MRUBY_LIB_DIR; build.rs must consume it.
    assert_match(/MRUBY_LIB_DIR/, contents,
                 "build.rs must reference the MRUBY_LIB_DIR env var that " \
                 "tasks/wasm.rake exports for Stage C linkage")
    assert_match(/cargo:rustc-link-search/, contents,
                 "build.rs must emit a cargo:rustc-link-search directive")
    assert_match(/cargo:rustc-link-lib=static=mruby/, contents,
                 "build.rs must request static linkage of libmruby.a")
  end

  def test_build_rs_skips_link_directives_off_wasm32
    contents = File.read(BUILD_RS)
    # The intent is captured by an early-return guard on
    # CARGO_CFG_TARGET_ARCH != "wasm32" so host `cargo test` (rlib build)
    # does not attempt to link against a non-existent host libmruby.a.
    assert_match(/CARGO_CFG_TARGET_ARCH/, contents,
                 "build.rs must guard link directives behind " \
                 "CARGO_CFG_TARGET_ARCH so host-target builds don't fail")
    assert_match(/wasm32/, contents,
                 "build.rs must compare CARGO_CFG_TARGET_ARCH against \"wasm32\"")
  end

  # ---- Syntax sanity checks --------------------------------------------

  def test_wasm_rake_is_syntactically_valid_ruby
    contents = File.read(WASM_RAKE)
    # RubyVM::InstructionSequence.compile raises SyntaxError on bad Ruby —
    # cheaper than spawning `ruby -c` and runs in-process.
    RubyVM::InstructionSequence.compile(contents, WASM_RAKE)
  rescue SyntaxError => e
    flunk "tasks/wasm.rake has a syntax error: #{e.message}"
  end

  def test_build_rs_is_syntactically_valid_rust
    # Lightweight structural check: build.rs must declare `fn main()`. A
    # full Rust parser is out of scope for the fast tier; the cargo check
    # tier in test_wasm_crate.rb covers semantic validity, and the real-
    # tier build (test_wasm_guest_build.rb) exercises the full compile.
    contents = File.read(BUILD_RS)
    assert_match(/fn\s+main\s*\(\s*\)/, contents,
                 "build.rs must declare a `fn main()` entry point")
  end
end
