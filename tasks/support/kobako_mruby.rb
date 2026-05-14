# frozen_string_literal: true

# mruby static-library build support module
# =========================================
#
# Pure-Ruby helpers backing +tasks/mruby.rake+. Owns the vendored
# +minirake+ invocation that produces the cross-compiled
# +libmruby.a+ (Stage B of the build pipeline). The .rake wrapper
# is the rake DSL surface that glues +KobakoMruby.invoke_minirake+
# to the +rake mruby:build+ task.

require "fileutils"
require "rbconfig"

# Stage B build helpers for the vendored mruby tree. See sibling
# +tasks/mruby.rake+ for the rake DSL.
module KobakoMruby
  ROOT          = File.expand_path("../..", __dir__)
  VENDOR_DIR    = (ENV["KOBAKO_VENDOR_DIR"] || File.join(ROOT, "vendor")).freeze
  MRUBY_DIR     = File.join(VENDOR_DIR, "mruby").freeze
  BUILD_CONFIG  = File.join(ROOT, "build_config", "wasi.rb").freeze

  # mruby places artefacts under `build/<target-name>/lib/libmruby.a`, where
  # `<target-name>` matches the `MRuby::Build.new(<name>)` argument in
  # `build_config/wasi.rb` (here: "wasi").
  TARGET_NAME   = "wasi"
  LIBMRUBY_PATH = File.join(MRUBY_DIR, "build", TARGET_NAME, "lib", "libmruby.a").freeze

  def self.minirake
    # mruby ships a vendored copy of `minirake` at the top of its tree.
    File.join(MRUBY_DIR, "minirake")
  end

  # Run mruby's minirake with our build config wired in via
  # MRUBY_CONFIG. mruby reads that env var (absolute path or basename
  # of a file under build_config/) to choose its top-level Build.
  def self.invoke_minirake(*args)
    env = { "MRUBY_CONFIG" => BUILD_CONFIG }
    cmd = [RbConfig.ruby, minirake, *args]
    puts "[mruby] cd #{MRUBY_DIR} && MRUBY_CONFIG=#{BUILD_CONFIG} #{cmd.join(" ")}"
    system(env, *cmd, chdir: MRUBY_DIR, exception: true)
  end
end
