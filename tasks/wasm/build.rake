# frozen_string_literal: true

# Stage C of the Build Pipeline — the Guest Binary artifact tasks.
#
#   * `rake wasm:build` — cross-compiles the kobako-wasm shell crate against
#                         the vendored wasi-sdk + libmruby.a and writes the
#                         Guest Binary to `data/kobako.wasm`. Runs
#                         `beni:build` (Stages A+B: toolchain vendoring +
#                         libmruby.a) first, so the full pipeline runs
#                         end-to-end from a clean clone with a single
#                         command; a current Guest Binary restored without
#                         the archive skips both.
#   * `rake wasm:clean` — removes every Guest Binary variant and the wasm
#                         crate's `target/` cache directory.
#
# The pure default and the two regexp variants share one GuestBuilder,
# parameterised by cargo features and output path. The host test and
# coverage tasks live in tasks/wasm/check.rake; shared helpers (paths,
# mtime idempotency, cargo env) in tasks/support/wasm.rb.

require_relative "../support/wasm"

# Build what +builder+ names, staging the mruby archive first where it
# needs one.
def build_guest(builder)
  Rake::Task["beni:build"].invoke if builder.needs_archive?
  KobakoWasm.ensure_cargo!
  builder.build
end

namespace :wasm do
  desc "Build Guest Binary (data/kobako.wasm) from kobako-wasm crate + libmruby.a (Stage C)"
  task :build do
    build_guest(KobakoWasm::GuestBuilder.new)
  end

  # One build task per capability variant, table-driven so the bodies do
  # not repeat. The matrix (task name => [features, output]) lives in
  # KobakoWasm so the build tasks, clean list, and check gate share it.
  KobakoWasm::VARIANT_BUILDS.each do |name, (features, output)|
    desc "Build the #{name.delete_prefix("build:")} variant Guest Binary (#{File.basename(output)})"
    task name do
      build_guest(KobakoWasm::GuestBuilder.new(features: features, output: output))
    end
  end

  # The sources every Guest Binary is built from, one path per line, so a
  # CI cache of the built binaries is keyed by what the build reads.
  task :sources do
    puts KobakoWasm::GuestBuilder.sources.map { |path| path.delete_prefix("#{KobakoWasm::ROOT}/") }.sort
  end

  desc "Remove every Guest Binary variant and the wasm crate target/ cache"
  task :clean do
    KobakoWasm::GUEST_BINARIES.each { |wasm| FileUtils.rm_f(wasm) }
    FileUtils.rm_rf(KobakoWasm::CRATE_TARGET_DIR)
    puts "[wasm:clean] removed the Guest Binary variants and #{KobakoWasm::CRATE_TARGET_DIR}"
  end
end
