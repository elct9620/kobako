# frozen_string_literal: true

require "bundler/gem_tasks"
require "minitest/test_task"

Minitest::TestTask.create

# Grouped subsets of the suite the main +test+ task already runs whole:
# the release-tooling readers and the bench-gate logic, runnable alone
# without the Guest Binary prerequisites +rake test+ chains.
namespace :test do
  desc "Run only the release-tooling unit suites (test/tasks/)."
  task :tasks do
    ruby "-Ilib", "-Itest", "-e", %(Dir["test/tasks/test_*.rb"].each { |f| require File.expand_path(f) })
  end

  desc "Run only the bench-gate unit suites (test/bench/)."
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

# Every variant a suite drives is a test prerequisite: the journey tests
# (test/e2e/test_journeys.rb) drive the pure data/kobako.wasm, the focused
# regexp suite (test/e2e/regexp/) the regexp variants, and the json suite
# (test/e2e/json/) the json variant — omitting one lets its whole suite skip
# silently under CI. All four are gitignored and mtime-idempotent, so this
# only does real work on a clean clone or when the wasm sources change.
task test: ["wasm:build", "wasm:build:regexp", "wasm:build:regexp_unicode", "wasm:build:json"]

# `gate` is the release gate's verification tier — the one place the
# gate:* checks are enumerated, so membership stays deliberate (a stray
# gate:*-named task never joins the release gate by accident). The default
# and CI reference `gate`, never the list.
desc "Run every gate:* verification check (the release gate's verification tier)."
task gate: %w[gate:rbs:lock gate:rbs:orphans gate:anchors gate:anchors:coverage gate:wire:symmetry
              gate:payload:optional gate:engine:optional gate:parity:coverage gate:surface
              gate:gvl:isolation gate:bench:smoke gate:release:wiring]

# `crates:test` joins the canonical gate because the core envelope's only
# pin lives there: the envelope has one implementation, so the golden
# vectors in `crates/kobako-transport` are what SPEC's Consistency
# guarantee rests on for that layer, and a default run that skipped them
# would report green on an unverified wire. The guest crates' own units
# stay in `wasm:test`, which carries no such claim.
task default: %i[compile test crates:test rubocop steep gate]
