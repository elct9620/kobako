# frozen_string_literal: true

require "bundler/gem_tasks"
require "minitest/test_task"

Minitest::TestTask.create

require "rubocop/rake_task"

RuboCop::RakeTask.new

require "rb_sys/extensiontask"

task build: :compile

GEMSPEC = Gem::Specification.load("kobako.gemspec")

RbSys::ExtensionTask.new("kobako", GEMSPEC) do |ext|
  ext.lib_dir = "lib/kobako"
end

# Load tasks/*.rake (vendor toolchain, build pipeline). Each .rake file is
# self-contained; see tasks/vendor.rake for the wasi-sdk / mruby fetch flow.
Dir.glob("tasks/*.rake").each { |t| load t }

# data/kobako.wasm is gitignored and required by Layer 4 journey tests
# (test/test_e2e_journeys.rb) plus the real-tier wasm wrapper check. The
# wasm:build task is idempotent (mtime short-circuit), so this only does
# real work on a clean clone or when the wasm crate source changes.
task test: "wasm:build"

task default: %i[compile test rubocop]
