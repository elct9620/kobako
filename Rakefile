# frozen_string_literal: true

require "bundler/gem_tasks"
require "minitest/test_task"

Minitest::TestTask.create

require "rubocop/rake_task"

RuboCop::RakeTask.new

require "steep/rake_task"

Steep::RakeTask.new

require "rb_sys/extensiontask"

# `bundler/gem_tasks` exposes `rake build` (and therefore `rake release`,
# which depends on it). `data/kobako.wasm` is gitignored, so we chain
# `wasm:build` here to guarantee the Guest Binary is present and fresh
# before the gem is packaged. `wasm:build` is mtime-idempotent, so this
# is free when the source tree hasn't changed.
task build: %i[compile wasm:build]

GEMSPEC = Gem::Specification.load("kobako.gemspec")

RbSys::ExtensionTask.new("kobako", GEMSPEC) do |ext|
  ext.lib_dir = "lib/kobako"
  # Enable `rake gem:<platform>` tasks so oxidize-rb/actions/cross-gem can
  # cross-compile precompiled native gems via rb-sys-dock.
  ext.cross_compile = true
end

require "beni/tasks"

# Stages A+B of the Build Pipeline: `rake beni:build` vendors the pinned
# wasi-sdk + mruby toolchains and drives mruby's own rake against
# build_config/wasi.rb, producing vendor/mruby/build/wasi/lib/libmruby.a
# (+ its libmruby.flags.mak sidecar). Only the wasi cross target is
# declared — the config's host build is mrbc-only, so there is no host
# libmruby.a for beni to verify.
Beni::Tasks.new do
  build_config "build_config/wasi.rb"

  target :wasi do
    toolchain "wasi-sdk"
  end
end

# Load tasks/**/*.rake (Stage C + bench/coverage wrappers). Each .rake file
# is self-contained; see tasks/wasm/ for the Guest Binary flow.
Dir.glob("tasks/**/*.rake").each { |t| load t }

# The journey tests (test/test_e2e_journeys.rb) drive the pure
# data/kobako.wasm; the focused regexp suite (test/regexp/) drives the
# regexp variants — the full surface on the unicode binary and the
# Unicode-gate rejection on the no-unicode one. All three are gitignored
# and mtime-idempotent, so this only does real work on a clean clone or
# when the wasm sources change.
task test: ["wasm:build", "wasm:build:regexp", "wasm:build:regexp_unicode"]

task default: %i[compile test rubocop steep anchors parity:coverage]
