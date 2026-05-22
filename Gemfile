# frozen_string_literal: true

source "https://rubygems.org"

# Specify your gem's dependencies in kobako.gemspec
gemspec

gem "rake", "~> 13.0"
gem "rake-compiler"

# Dev-only tooling. The rb-sys-dock build image ships Ruby 3.1.3 for
# orchestrating cross-compilation, while steep ~> 2.0 demands Ruby >= 3.2.
# Isolating the dev tools behind a group lets the release workflow set
# BUNDLE_WITHOUT=development so the container's bundler never has to
# resolve them. Local `bundle install` keeps installing everything.
group :development do
  gem "irb"
  gem "minitest", "~> 6.0"
  gem "rubocop", "~> 1.86"

  # Static type checker. Signatures live in sig/.
  gem "steep", "~> 2.0", require: false

  # webrick is no longer bundled with Ruby 3.0+; the vendor:setup E2E test
  # spins up a tiny HTTP fixture server to serve fake tarballs.
  gem "webrick", "~> 1.8"

  # benchmark-ips drives the SPEC.md "Regression benchmarks" suite in
  # benchmark/. Dev-only — the gem itself does not depend on it.
  gem "benchmark-ips", "~> 2.14"
end
