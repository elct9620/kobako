# frozen_string_literal: true

require_relative "lib/kobako/version"

Gem::Specification.new do |spec|
  spec.name = "kobako"
  spec.version = Kobako::VERSION
  spec.authors = ["Aotokitsuruya"]
  spec.email = ["contact@aotoki.me"]

  spec.summary = "Embeddable Wasm sandbox for running untrusted mruby code from Ruby applications."
  spec.description = "kobako provides an in-process Wasm sandbox (wasmtime + mruby) with a MessagePack-based host/guest RPC, allowing Ruby applications to execute untrusted mruby scripts under capability-based Service injection."
  spec.homepage = "https://github.com/elct9620/kobako"
  spec.license = "Apache-2.0"
  spec.required_ruby_version = ">= 3.2.0"
  spec.required_rubygems_version = ">= 3.3.11"

  spec.metadata["allowed_push_host"] = "https://rubygems.org"
  spec.metadata["homepage_uri"] = spec.homepage
  spec.metadata["source_code_uri"] = "https://github.com/elct9620/kobako"
  spec.metadata["changelog_uri"] = "https://github.com/elct9620/kobako/blob/main/CHANGELOG.md"
  spec.metadata["bug_tracker_uri"] = "https://github.com/elct9620/kobako/issues"

  # Explicit allowlist for files shipped in the published gem. Keep this in
  # sync with SPEC.md "Code Organization" §gemspec files whitelist.
  #
  # Included: lib/, ext/kobako/ source, data/kobako.wasm, Rakefile, the
  # gemspec itself, README.md, LICENSE, CHANGELOG.md.
  #
  # Excluded (must NOT ship): vendor/, wasm/, tasks/, build_config/, docs/,
  # benchmark/, test/ (or spec/), bin/, .github/, .powerloop/, tmp/, Gemfile,
  # .rubocop.yml, .gitignore, SPEC.md, Cargo.toml (workspace root).
  spec.files = Dir.chdir(__dir__) do
    Dir.glob("lib/**/*.rb") +
      Dir.glob("ext/kobako/**/*.{rs,toml,rb,h}") +
      %w[data/kobako.wasm Rakefile kobako.gemspec README.md LICENSE CHANGELOG.md]
      .select { |f| File.exist?(f) }
  end
  spec.bindir = "exe"
  spec.executables = []
  spec.require_paths = ["lib"]
  spec.extensions = ["ext/kobako/extconf.rb"]

  # Uncomment to register a new dependency of your gem
  # spec.add_dependency "example-gem", "~> 1.0"
  spec.add_dependency "rb_sys", "~> 0.9.91"

  # For more information and examples about making a new gem, check out our
  # guide at: https://bundler.io/guides/creating_gem.html
end
