# frozen_string_literal: true

require "bundler/gem_tasks"
require "minitest/test_task"

Minitest::TestTask.create

# Grouped subsets of the suite the main +test+ task already runs whole:
# the release tooling and the benchmarks, runnable alone without the Guest
# Binary prerequisites +rake test+ chains; a test needing one skips.
namespace :test do
  desc "Run only the release-tooling suites (test/tasks/)."
  task :tasks do
    ruby "-Ilib", "-Itest", "-e", %(Dir["test/tasks/test_*.rb"].each { |f| require File.expand_path(f) })
  end

  desc "Run only the benchmark suites (test/bench/)."
  task :bench do
    ruby "-Ilib", "-Itest", "-e", %(Dir["test/bench/test_*.rb"].each { |f| require File.expand_path(f) })
  end
end

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
# (+ its libmruby.flags.mak sidecar) for the Guest Binary, and the host
# archive the guest crates' host-target unit tests link against.
# Windows builds no Guest Binary, so it vendors no wasi-sdk and stages the
# host archive alone; CI hands it the Guest Binaries another host built.
Beni::Tasks.new do
  build_config "build_config/wasi.rb"

  target :host
  unless Gem.win_platform?
    target :wasi do
      toolchain "wasi-sdk"
    end
  end
end

# Load tasks/**/*.rake (Stage C + bench/coverage wrappers). Each .rake file
# is self-contained; see tasks/wasm/ for the Guest Binary flow.
Dir.glob("tasks/**/*.rake").each { |t| load t }

# Every variant a suite drives is a test prerequisite: the journey tests
# (test/e2e/test_journeys.rb) drive the pure data/kobako.wasm, the focused
# regexp suite (test/e2e/regexp/) the regexp variants and the full one, and
# the json suite (test/e2e/json/) the json variant — omitting one lets its
# tests skip silently under CI. All are gitignored and mtime-idempotent, so
# this only does real work on a clean clone or when the wasm sources change.
# The oracle tests build guest crates on the host, linking the mruby archive
# beni:build stages.
task test: ["beni:build", "wasm:build", "wasm:build:regexp", "wasm:build:regexp_unicode", "wasm:build:json",
            "wasm:build:full"]

# `crates:test` joins the default because the core envelope's only pin
# lives there: the envelope has one implementation, so the golden vectors
# in `crates/kobako-transport` are what the wire's consistency guarantee
# rests on for that layer, and a default run that skipped them would
# report green on an unverified wire. The guest crates' own units stay in
# `wasm:test`, which carries no such claim. `crates:graph` holds what no
# test binary can: the build graphs the published crates promise.
task default: %i[compile test crates:test crates:graph rubocop steep]
