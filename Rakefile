# frozen_string_literal: true

require "bundler/gem_tasks"
require "minitest/test_task"

Minitest::TestTask.create

require "rubocop/rake_task"

RuboCop::RakeTask.new

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
end

# Load tasks/*.rake (vendor toolchain, build pipeline). Each .rake file is
# self-contained; see tasks/vendor.rake for the wasi-sdk / mruby fetch flow.
Dir.glob("tasks/*.rake").each { |t| load t }

# data/kobako.wasm is gitignored and required by Layer 4 journey tests
# (test/test_e2e_journeys.rb). The wasm:build task is idempotent (mtime
# short-circuit), so this only does real work on a clean clone or when
# the wasm crate source changes.
task test: "wasm:build"

task default: %i[compile test rubocop]
