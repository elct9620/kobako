# frozen_string_literal: true

source "https://rubygems.org"

# Specify your gem's dependencies in kobako.gemspec
gemspec

gem "irb"
gem "rake", "~> 13.0"

gem "rake-compiler"

gem "minitest", "~> 5.16"

gem "rubocop", "~> 1.21"

# webrick is no longer bundled with Ruby 3.0+; the vendor:setup E2E test
# spins up a tiny HTTP fixture server to serve fake tarballs.
gem "webrick", "~> 1.8"
