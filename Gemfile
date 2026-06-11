# frozen_string_literal: true

source "https://rubygems.org"

# Specify your gem's dependencies in kobako.gemspec
gemspec

gem "rake", "~> 13.0"
gem "rake-compiler"

# Vendors the pinned mruby + wasi-sdk toolchains and builds libmruby.a
# (rake beni:build) against build_config/wasi.rb. Top-level rather than
# development-grouped because the Rakefile requires "beni/tasks"
# unconditionally, same as rake-compiler above. Pinned to the 0.2
# series — the wasm/ workspace's `beni` crate pin is `< 0.3.0` and the
# gem + crates release in lockstep, so the two sides must move together.
gem "beni", "~> 0.2.0"

# Dev-only tooling, grouped so a constrained environment can exclude it
# via BUNDLE_WITHOUT=development. The rb-sys-dock cross-compile container
# currently resolves the full Gemfile (rake-compiler-dock 1.12 ships
# Ruby 4.0), so the group is a boundary, not a workaround.
group :development do
  gem "irb"
  gem "minitest", "~> 6.0"
  gem "rubocop", "~> 1.87"

  # Static type checker. Signatures live in sig/.
  gem "steep", "~> 2.0", require: false

  # benchmark-ips drives the SPEC.md "Regression benchmarks" suite in
  # benchmark/. Dev-only — the gem itself does not depend on it.
  gem "benchmark-ips", "~> 2.15"
end
