# frozen_string_literal: true

$LOAD_PATH.unshift File.expand_path("../lib", __dir__)

# Loading kobako requires the native ext (lib/kobako/kobako.bundle on darwin).
# In clean checkouts before `bundle exec rake compile` runs, that bundle is
# absent — degrade gracefully so individual test files can `skip` rather
# than the whole test suite blowing up at require time. Tests that need the
# native ext check `defined?(Kobako::Runtime)` and skip.
begin
  require "kobako"
rescue LoadError => e
  warn "[test_helper] kobako native ext not loadable: #{e.message}"
  warn "[test_helper] tests requiring the ext will be skipped; run `bundle exec rake compile` to enable them"

  # Only `kobako/kobako` (the ext) and `kobako/runtime` (which reopens the
  # ext-defined Kobako::Runtime) need the compiled bundle. Load the rest of
  # the pure-Ruby tree by hand so codec / transport / catalog / outcome unit
  # tests still run on a clean checkout. Kobako::Runtime and Kobako::Snapshot
  # stay undefined on purpose — the ext-dependent tests skip on
  # `defined?(Kobako::Runtime)` / `defined?(Kobako::Snapshot)`.
  require "kobako/version"
  require "kobako/errors"
  require "kobako/outcome"
  require "kobako/transport"
  require "kobako/catalog"
  require "kobako/sandbox"
end

require "stringio"
require "msgpack"

require "minitest/autorun"
require_relative "support/outcome_bytes_helpers"
require_relative "support/cargo_oracle"
require_relative "support/wire_value_generator"
