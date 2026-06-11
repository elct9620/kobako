# frozen_string_literal: true

# Standalone regexp characterization benchmark (#10; not in the release gate).
# It lives in its own task file — rake reopens the shared :bench namespace
# across files — so the regexp capability profile is not folded into the
# general run.rake roster. The script writes its suite into
# benchmark/results/<date>-<short-sha>.json like the other benchmarks.
namespace :bench do
  desc "Run the regexp compile-vs-match + operations characterization (#10; not in release gate)."
  task :regexp do
    sh "bundle exec ruby benchmark/regexp.rb"
  end
end
