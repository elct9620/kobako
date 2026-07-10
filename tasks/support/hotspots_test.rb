# frozen_string_literal: true

require "minitest/autorun"

require_relative "hotspots"

# Unit coverage for the hotspot scorer: churn parses only tracked source
# trees out of the git log stream, fan-in follows require_relative edges
# to root-relative paths, and rows rank by churn × size with vanished
# files dropped.
class KobakoHotspotsTest < Minitest::Test
  Hotspots = KobakoHotspots

  def test_churn_counts_source_files_and_ignores_other_paths
    log = <<~LOG
      lib/kobako/sandbox.rb
      lib/kobako/sandbox.rb

      docs/wire-contract.md
      test/sandbox/test_run.rb
      crates/kobako-codec/src/codec.rs
    LOG

    churn = Hotspots.churn(log)

    assert_equal({ "lib/kobako/sandbox.rb" => 2, "crates/kobako-codec/src/codec.rs" => 1 }, churn,
                 "docs and test paths must stay outside the hotspot churn")
  end

  def test_fan_in_resolves_require_relative_to_root_relative_paths
    sources = {
      "lib/kobako/sandbox.rb" => 'require_relative "codec"',
      "lib/kobako/catalog/handles.rb" => 'require_relative "../handle"'
    }

    fan_in = Hotspots.fan_in(sources)

    assert_equal 1, fan_in["lib/kobako/codec.rb"]
    assert_equal 1, fan_in["lib/kobako/handle.rb"],
                 "a ../-relative require must resolve against the requiring file's directory"
  end

  def test_rows_rank_by_churn_times_size_and_drop_vanished_files
    rows = Hotspots.rows(
      churn: { "lib/a.rb" => 10, "lib/b.rb" => 2, "lib/gone.rb" => 99 },
      sizes: { "lib/a.rb" => 10, "lib/b.rb" => 500 },
      fan_in: { "lib/b.rb" => 3 }
    )

    assert_equal [["lib/b.rb", 2, 500, 3], ["lib/a.rb", 10, 10, 0]], rows,
                 "a deleted file must not appear even with the highest churn"
  end

  def test_rows_honor_the_limit
    churn = { "lib/a.rb" => 2, "lib/b.rb" => 1 }
    sizes = { "lib/a.rb" => 10, "lib/b.rb" => 10 }

    assert_equal 1, Hotspots.rows(churn: churn, sizes: sizes, fan_in: {}, limit: 1).size
  end
end
