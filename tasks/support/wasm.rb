# frozen_string_literal: true

# wasm Rust crate (kobako-wasm) support module
# ============================================
#
# Pure-Ruby helpers backing +tasks/wasm/+. Owns crate paths, the
# wasm32-wasip1 target name, and the Stage C orchestrator. The
# .rake wrapper is the rake DSL surface that glues these helpers to
# +rake wasm:test+ / +rake wasm:build+.

# Stage C build helpers for the kobako-wasm crate. See
# +tasks/wasm/+ for the rake DSL and +KobakoWasm::GuestBuilder+ for
# the orchestrator class.
module KobakoWasm
  ROOT = File.expand_path("../..", __dir__)
  # `wasm/` is a cargo sub-workspace whose members share a single
  # `target/` directory at the workspace root. `kobako-wasm` is the
  # cdylib-bearing shell; its sibling `kobako-core` is a path
  # dependency with no separate artifact, and the mruby wrapper comes
  # from the published `beni` crate.
  WASM_WORKSPACE_DIR = File.join(ROOT, "wasm").freeze
  CRATE_DIR  = File.join(WASM_WORKSPACE_DIR, "kobako-wasm").freeze
  MANIFEST   = File.join(CRATE_DIR, "Cargo.toml").freeze
  WASM_TARGET = "wasm32-wasip1"

  # Stage C output paths. Cargo derives the workspace-shared target
  # directory from the workspace root rather than the cdylib member.
  CRATE_TARGET_DIR  = File.join(WASM_WORKSPACE_DIR, "target").freeze
  CRATE_WASM_OUTPUT = File.join(CRATE_TARGET_DIR, WASM_TARGET, "release", "kobako_wasm.wasm").freeze

  DATA_DIR  = File.join(ROOT, "data").freeze
  DATA_WASM = File.join(DATA_DIR, "kobako.wasm").freeze

  # Regexp-capability Guest Binary variants. `+` separates the base
  # binary from an appended capability; the capability token itself uses
  # `-` (`regexp`, `regexp-unicode`). Built by `wasm:build:regexp` /
  # `wasm:build:regexp_unicode` and shipped as downloadable Release
  # assets — never bundled into the gem.
  DATA_WASM_REGEXP         = File.join(DATA_DIR, "kobako+regexp.wasm").freeze
  DATA_WASM_REGEXP_UNICODE = File.join(DATA_DIR, "kobako+regexp-unicode.wasm").freeze

  # JSON-capability Guest Binary variants. `json` carries kobako-json
  # alone; `full` composes ASCII regexp with json. Built by
  # `wasm:build:json` / `wasm:build:full` and shipped as downloadable
  # Release assets — never bundled into the gem.
  DATA_WASM_JSON = File.join(DATA_DIR, "kobako+json.wasm").freeze
  DATA_WASM_FULL = File.join(DATA_DIR, "kobako+full.wasm").freeze

  # Variant build matrix: rake task name => [cargo features, output path].
  # The single source of truth for the capability variant set — drives the
  # wasm:build:<variant> tasks and the wasm:clean artifact list.
  VARIANT_BUILDS = {
    "build:regexp" => [["regexp"], DATA_WASM_REGEXP],
    "build:regexp_unicode" => [["regexp-unicode"], DATA_WASM_REGEXP_UNICODE],
    "build:json" => [["json"], DATA_WASM_JSON],
    "build:full" => [["full"], DATA_WASM_FULL]
  }.freeze

  # Every Guest Binary artifact, default plus variants — the set wasm:clean
  # removes.
  GUEST_BINARIES = [DATA_WASM, *VARIANT_BUILDS.values.map(&:last)].freeze

  # Stage B output (produced by `rake beni:build` against
  # build_config/wasi.rb). The vendor base mirrors the `Beni::Tasks`
  # default (`BENI_VENDOR_DIR` or `vendor/` at the project root) so the
  # Stage C exports name the same tree beni populated.
  VENDOR_DIR     = (ENV["BENI_VENDOR_DIR"] || File.join(ROOT, "vendor")).freeze
  WASI_SDK_DIR   = (ENV["WASI_SDK_PATH"] || File.join(VENDOR_DIR, "wasi-sdk")).freeze
  MRUBY_LIB_DIR  = File.join(VENDOR_DIR, "mruby", "build", "wasi", "lib").freeze
  LIBMRUBY_PATH  = File.join(MRUBY_LIB_DIR, "libmruby.a").freeze

  # Environment for a host-target cargo run over the `wasm/` workspace:
  # `beni-sys` discovers the host archive through the vendor tree
  # `rake beni:build` staged (the `host` build of build_config/wasi.rb).
  HOST_CARGO_ENV = { "BENI_VENDOR_DIR" => VENDOR_DIR }.freeze

  def self.cargo_available?
    system("which cargo > /dev/null 2>&1")
  end

  # True when the `cargo llvm-cov` subcommand is installed. It is an
  # optional add-on (`cargo install cargo-llvm-cov`), not part of a base
  # toolchain, so the coverage tasks gate on it separately from cargo.
  def self.llvm_cov_available?
    system("cargo llvm-cov --version > /dev/null 2>&1")
  end

  # Shared guard for the Stage C build tasks: abort with an install hint
  # when cargo is absent, so each task body stays a single build call.
  def self.ensure_cargo!
    abort "cargo not on PATH; install the Rust toolchain to build the Guest Binary" unless cargo_available?
  end

  # Shared guard for the Rust coverage tasks: abort with a targeted
  # install hint when cargo or the llvm-cov add-on is missing, so each
  # coverage task body stays a single measurement call.
  def self.ensure_llvm_cov!
    abort "cargo not on PATH; install the Rust toolchain to measure Rust coverage" unless cargo_available?
    return if llvm_cov_available?

    abort "cargo llvm-cov not installed; run `cargo install cargo-llvm-cov` to measure Rust coverage"
  end
end

require_relative "wasm/baker"
require_relative "wasm/guest_builder"
