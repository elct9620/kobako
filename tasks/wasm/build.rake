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

  # One build task per capability variant, table-driven so the bodies do
  # not repeat. The matrix (task name => [features, output]) lives in
  # KobakoWasm so the build tasks, clean list, and check gate share it.
  KobakoWasm::VARIANT_BUILDS.each do |name, (features, output)|
    desc "Build the #{name.delete_prefix("build:")} variant Guest Binary (#{File.basename(output)})"
    task name => ["beni:build"] do
      KobakoWasm.ensure_cargo!
      KobakoWasm::GuestBuilder.new(features: features, output: output).build
    end
  end

  desc "Remove every Guest Binary variant and the wasm crate target/ cache"
  task :clean do
    KobakoWasm::GUEST_BINARIES.each { |wasm| FileUtils.rm_f(wasm) }
    FileUtils.rm_rf(KobakoWasm::CRATE_TARGET_DIR)
    puts "[wasm:clean] removed the Guest Binary variants and #{KobakoWasm::CRATE_TARGET_DIR}"
  end
end
