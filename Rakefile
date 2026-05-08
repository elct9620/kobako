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

task default: %i[compile test rubocop]
