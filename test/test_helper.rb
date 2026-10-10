# frozen_string_literal: true

$LOAD_PATH.unshift File.expand_path("../lib", __dir__)

# A clean checkout has no native ext until `bundle exec rake compile`, so the
# pure-Ruby tree loads on its own and the tests that need the ext skip on
# `defined?(Kobako::Runtime)`.
begin
  require "kobako"
rescue LoadError => e
  warn "[test_helper] kobako native ext not loadable: #{e.message}"
  warn "[test_helper] tests requiring the ext will be skipped; run `bundle exec rake compile` to enable them"

  # `kobako/sandbox` requires the whole pure-Ruby tree but not the
  # ext-defined Kobako::Runtime, so it loads without the bundle and needs no
  # list kept in sync with lib/kobako.rb.
  require "kobako/version"
  require "kobako/sandbox"
  # Pool sits above the sandbox aggregator (the checkout layer), so it is
  # not in sandbox.rb's require graph and loads explicitly.
  require "kobako/pool"
end

# stringio is not part of the kobako load graph; tests that capture IO
# (test/e2e/sandbox/test_run_auto_wrap.rb) needs it
# explicitly. msgpack is
# intentionally not required here — kobako's codec already pulls it in, so
# the few tests using MessagePack directly get it through that graph.
require "stringio"

require "minitest/autorun"
require_relative "support/paths"
require_relative "support/guest_guard"
require_relative "support/cargo_oracle"
require_relative "support/wire_value_generator"
require_relative "support/dispatch_program_generator"
require_relative "support/regexp_helper"
require_relative "support/json_helper"
require_relative "support/e2e_helper"
require_relative "support/codec_helpers"
require_relative "support/dispatcher_helpers"
require_relative "support/parity"
