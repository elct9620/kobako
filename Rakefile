# frozen_string_literal: true

require "bundler/gem_tasks"
require "minitest/test_task"

Minitest::TestTask.create

# Grouped subsets of the suite the main +test+ task already runs whole:
# the release tooling and the benchmarks, runnable alone; a test needing a
# Guest Binary skips.
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
# before the gem is packaged. Cargo recompiles only what changed, so this
# costs little when the source tree hasn't.
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

# The checks split as the build does: the guest's need the mruby archive
# beni stages, the host's need only the Guest Binaries, so a host given
# them by another, as CI's build jobs are, never stages the archive.
namespace :check do
  # Every variant a suite drives is built: the journey tests
  # (test/e2e/test_journeys.rb) drive the pure data/kobako.wasm, the
  # focused regexp suite (test/e2e/regexp/) the regexp variants and the
  # full one, and the json suite (test/e2e/json/) the json variant —
  # omitting one leaves its tests to fail under CI. Cargo recompiles only
  # what changed, so an unchanged tree costs a fingerprint check and a bake.
  desc "Build every Guest Binary and check the guest crates"
  task guest: ["wasm:build", "wasm:build:regexp", "wasm:build:regexp_unicode", "wasm:build:json",
               "wasm:build:full", "wasm:test", "wasm:graph"]

  # `crates:test` belongs here because the core envelope's only pin lives
  # there: the envelope has one implementation, so the golden vectors in
  # `crates/kobako-transport` are what the wire's consistency guarantee
  # rests on for that layer. `crates:graph` holds what no test binary can:
  # the build graphs the published host crates promise.
  desc "Check the host against the Guest Binaries already built"
  task host: %i[compile test crates:test crates:graph rubocop steep]
end

task default: %w[check:guest check:host]
