# frozen_string_literal: true

# mruby static-library build task (Stage B of the Build Pipeline).
#
# Drives mruby's bundled `minirake` against `build_config/wasi.rb`, producing
# the cross-compiled `libmruby.a` that the wasm crate (Stage C) links into
# the guest binary. This task is the single, idempotent entry point:
#
#   $ rake mruby:build      # produces vendor/mruby/build/wasi/lib/libmruby.a
#   $ rake mruby:clean      # removes mruby's build/wasi/ tree
#
# Depends on `vendor:setup` (tasks/vendor.rake), so the wasi-sdk + mruby
# tarballs are present before mruby's minirake fires its first compile.
# Idempotency: the underlying minirake is itself a make-style incremental
# build; on top of that, this task short-circuits when the libmruby.a
# sentinel already exists, so a second `rake mruby:build` invocation is a
# no-op without even invoking minirake.
#
# Stage B paths and the +invoke_minirake+ helper live in
# tasks/support/kobako_mruby.rb.

require_relative "support/kobako_mruby"

namespace :mruby do
  desc "Build vendored mruby for wasm32-wasip1 (produces #{KobakoMruby::LIBMRUBY_PATH})"
  task build: ["vendor:setup"] do
    if File.exist?(KobakoMruby::LIBMRUBY_PATH)
      puts "[mruby] libmruby.a already present at #{KobakoMruby::LIBMRUBY_PATH} — skipping"
      next
    end

    KobakoMruby.invoke_minirake

    unless File.exist?(KobakoMruby::LIBMRUBY_PATH)
      raise "[mruby] build completed but #{KobakoMruby::LIBMRUBY_PATH} is missing"
    end

    puts "[mruby] libmruby.a ready at #{KobakoMruby::LIBMRUBY_PATH}"
  end

  desc "Remove mruby's build/wasi/ tree (keeps vendored mruby source)"
  task :clean do
    build_dir = File.join(KobakoMruby::MRUBY_DIR, "build", KobakoMruby::TARGET_NAME)
    FileUtils.rm_rf(build_dir)
    puts "[mruby] removed #{build_dir}"
  end
end
