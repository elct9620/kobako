# frozen_string_literal: true

# Stage C of the Build Pipeline — the Guest Binary artifact tasks.
#
#   * `rake wasm:build` — cross-compiles the kobako-wasm shell crate against
#                         the vendored wasi-sdk + libmruby.a and writes the
#                         Guest Binary to `data/kobako.wasm`. Depends on
#                         `beni:build` (Stages A+B: toolchain vendoring +
#                         libmruby.a) so the full pipeline runs end-to-end
#                         from a clean clone with a single command.
#   * `rake wasm:clean` — removes every Guest Binary variant and the wasm
#                         crate's `target/` cache directory.
#
# The pure default and the two regexp variants share one GuestBuilder,
# parameterised by cargo features and output path. The compile-only signal
# tasks (check / test) live in tasks/wasm/check.rake; shared helpers
# (paths, target detection, mtime idempotency, cargo env) in
# tasks/support/wasm.rb.

require_relative "../support/wasm"

namespace :wasm do
  desc "Build Guest Binary (data/kobako.wasm) from kobako-wasm crate + libmruby.a (Stage C)"
  task build: ["beni:build"] do
    KobakoWasm.ensure_cargo!
    KobakoWasm::GuestBuilder.new.build
  end

  desc "Build the regexp variant Guest Binary (data/kobako+regexp.wasm; no Unicode)"
  task "build:regexp" => ["beni:build"] do
    KobakoWasm.ensure_cargo!
    KobakoWasm::GuestBuilder.new(features: ["regexp"], output: KobakoWasm::DATA_WASM_REGEXP).build
  end

  desc "Build the regexp+unicode variant Guest Binary (data/kobako+regexp-unicode.wasm)"
  task "build:regexp_unicode" => ["beni:build"] do
    KobakoWasm.ensure_cargo!
    KobakoWasm::GuestBuilder.new(features: ["regexp-unicode"], output: KobakoWasm::DATA_WASM_REGEXP_UNICODE).build
  end

  desc "Build the json variant Guest Binary (data/kobako+json.wasm)"
  task "build:json" => ["beni:build"] do
    KobakoWasm.ensure_cargo!
    KobakoWasm::GuestBuilder.new(features: ["json"], output: KobakoWasm::DATA_WASM_JSON).build
  end

  desc "Build the full variant Guest Binary (data/kobako+full.wasm; ASCII regexp + json)"
  task "build:full" => ["beni:build"] do
    KobakoWasm.ensure_cargo!
    KobakoWasm::GuestBuilder.new(features: ["full"], output: KobakoWasm::DATA_WASM_FULL).build
  end

  desc "Remove every Guest Binary variant and the wasm crate target/ cache"
  task :clean do
    [
      KobakoWasm::DATA_WASM,
      KobakoWasm::DATA_WASM_REGEXP,
      KobakoWasm::DATA_WASM_REGEXP_UNICODE,
      KobakoWasm::DATA_WASM_JSON,
      KobakoWasm::DATA_WASM_FULL
    ].each do |wasm|
      FileUtils.rm_f(wasm)
    end
    FileUtils.rm_rf(KobakoWasm::CRATE_TARGET_DIR)
    puts "[wasm:clean] removed the Guest Binary variants and #{KobakoWasm::CRATE_TARGET_DIR}"
  end
end
