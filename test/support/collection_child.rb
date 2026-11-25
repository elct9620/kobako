# frozen_string_literal: true

# A child process whose Ruby collects garbage by thresholds the case sets,
# through the real guest binary. When Ruby collects on its own depends on how
# much the process allocated before, so a case judged by collection runs in a
# child of its own with those thresholds fixed, as CRuby's own GC tests run
# under child_env. Every Ruby from 3.3 reads these variables the same way.
module CollectionChild
  include ChildProcess

  DEADLINE = 60

  # Thresholds no run reaches, so a collection during one is one asked for.
  PAST_ANY_RUN = {
    "RUBY_GC_MALLOC_LIMIT" => "4000000000",
    "RUBY_GC_MALLOC_LIMIT_MAX" => "4000000000",
    "RUBY_GC_OLDMALLOC_LIMIT" => "4000000000",
    "RUBY_GC_OLDMALLOC_LIMIT_MAX" => "4000000000",
    "RUBY_GC_HEAP_0_INIT_SLOTS" => "2000000",
    "RUBY_GC_HEAP_1_INIT_SLOTS" => "1000000",
    "RUBY_GC_HEAP_2_INIT_SLOTS" => "500000",
    "RUBY_GC_HEAP_3_INIT_SLOTS" => "200000",
    "RUBY_GC_HEAP_4_INIT_SLOTS" => "100000"
  }.freeze

  # Thresholds of 16 MiB that never grow, so what native memory reports alone
  # decides when Ruby collects.
  BY_REPORTED_MEMORY = {
    "RUBY_GC_MALLOC_LIMIT" => (16 << 20).to_s,
    "RUBY_GC_MALLOC_LIMIT_MAX" => (16 << 20).to_s,
    "RUBY_GC_OLDMALLOC_LIMIT" => (16 << 20).to_s,
    "RUBY_GC_OLDMALLOC_LIMIT_MAX" => (16 << 20).to_s
  }.freeze

  PRELUDE = <<~RUBY
    require "kobako"
    WASM = ENV.fetch("KOBAKO_WASM")
  RUBY

  # What +script+ prints, run after the prelude with +args+ in a child
  # collecting by +thresholds+.
  def run_collection_child(script, *, thresholds:)
    run_ruby(PRELUDE + script, *, env: thresholds.merge("KOBAKO_WASM" => E2eGuestHelper::REAL_WASM),
                                  deadline: DEADLINE)
  end
end
