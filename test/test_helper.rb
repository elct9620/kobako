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
  # ext-defined Kobako::Runtime) need the compiled bundle. `kobako/sandbox`
  # is the aggregator that transitively requires the whole pure-Ruby tree
  # (errors / outcome / transport / catalog / codec), so loading it here lets
  # codec / transport / catalog / outcome unit tests still run on a clean
  # checkout — and a new pure-Ruby module wired into that graph is picked up
  # automatically, no list to keep in sync with lib/kobako.rb. Kobako::Runtime
  # stays undefined on purpose (sandbox.rb does not pull it), so the
  # ext-dependent tests skip on `defined?(Kobako::Runtime)`.
  require "kobako/version"
  require "kobako/sandbox"
  # Pool sits above the sandbox aggregator (the checkout layer), so it is
  # not in sandbox.rb's require graph and loads explicitly.
  require "kobako/pool"
end

# stringio is not part of the kobako load graph; tests that capture IO
# (test/codec/test_utils.rb / test/sandbox/test_run_auto_wrap.rb) need it
# explicitly. msgpack is
# intentionally not required here — kobako's codec already pulls it in, so
# the few tests using MessagePack directly get it through that graph.
require "stringio"

require "minitest/autorun"
require_relative "support/outcome_bytes_helpers"
require_relative "support/cargo_oracle"
require_relative "support/wire_value_generator"
require_relative "support/regexp_helper"
require_relative "support/json_helper"
require_relative "support/e2e_helper"
require_relative "support/codec_helpers"
require_relative "support/dispatcher_helpers"
require_relative "support/parity"
