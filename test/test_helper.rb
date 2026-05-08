# frozen_string_literal: true

$LOAD_PATH.unshift File.expand_path("../lib", __dir__)

# Loading kobako requires the native ext (lib/kobako/kobako.bundle on darwin).
# In clean checkouts before `bundle exec rake compile` runs, that bundle is
# absent — degrade gracefully so individual test files can `skip` rather
# than the whole test suite blowing up at require time. Tests that need the
# native ext check `defined?(Kobako::Wasm::Engine)` (or similar) and skip.
begin
  require "kobako"
rescue LoadError => e
  warn "[test_helper] kobako native ext not loadable: #{e.message}"
  warn "[test_helper] tests requiring the ext will be skipped; run `bundle exec rake compile` to enable them"
  require "kobako/version"
  module Kobako
    class Error < StandardError; end
  end
end

require "minitest/autorun"
