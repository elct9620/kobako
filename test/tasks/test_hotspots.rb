# frozen_string_literal: true

require "test_helper"

require_relative "../../tasks/support/hotspots"

# Unit coverage for the hotspot scorer: churn parses only tracked source
# trees out of the git log stream, fan-in follows require_relative edges
# to root-relative paths, and rows rank by churn × size with vanished
# files dropped.
class KobakoHotspotsTest < Minitest::Test
  Hotspots = KobakoHotspots

  # A git-log --name-only stream mixing every tier the churn scan must
  # include with the doc and test paths it must not.
  CHURN_LOG = <<~LOG
    lib/kobako/sandbox.rb
    lib/kobako/sandbox.rb

    docs/wire-contract.md
    test/sandbox/test_run.rb
    crates/kobako-codec/src/codec.rs
    tasks/hotspots.rake
    benchmark/support/gate.rb
  LOG

  def test_churn_counts_source_files_and_ignores_other_paths
    expected = { "lib/kobako/sandbox.rb" => 2, "crates/kobako-codec/src/codec.rs" => 1,
                 "tasks/hotspots.rake" => 1, "benchmark/support/gate.rb" => 1 }

    churn = Hotspots.churn(CHURN_LOG, roots: %w[lib crates tasks benchmark])

    assert_equal expected, churn,
                 "the tooling tiers churn like any source tree; docs and test paths stay outside"
  end

  def test_fan_in_resolves_require_relative_to_root_relative_paths
    sources = {
      "lib/kobako/sandbox.rb" => 'require_relative "codec"',
      "lib/kobako/catalog/handles.rb" => 'require_relative "../handle"'
    }

    fan_in = Hotspots.fan_in(sources)

    assert_equal 1, fan_in["lib/kobako/codec.rb"],
                 "a sibling require through fan_in must count toward the required file's path"
    assert_equal 1, fan_in["lib/kobako/handle.rb"],
                 "a ../-relative require must resolve against the requiring file's directory"
  end

  # The zero/unmeasured split: a scanned source nobody requires reads 0,
  # while a path outside the scan stays absent so the report can render
  # it as unmeasured instead of "no dependents".
  def test_fan_in_reports_zero_for_scanned_sources_and_omits_unscanned_paths
    fan_in = Hotspots.fan_in({ "lib/kobako/sandbox.rb" => "# no requires" })

    assert_equal 0, fan_in.fetch("lib/kobako/sandbox.rb"),
                 "a scanned file with no dependents must read fan-in 0, not unmeasured"
    assert_nil fan_in["crates/kobako-codec/src/codec.rs"],
               "a file outside the require_relative scan must read unmeasured, not fan-in 0"
  end

  def test_rows_rank_by_churn_times_size_and_drop_vanished_files
    rows = Hotspots.rows(
      churn: { "lib/a.rb" => 10, "lib/b.rb" => 2, "lib/gone.rb" => 99 },
      sizes: { "lib/a.rb" => 10, "lib/b.rb" => 500 },
      fan_in: { "lib/b.rb" => 3 }
    )

    assert_equal [["lib/b.rb", 2, 500, 3], ["lib/a.rb", 10, 10, nil]], rows,
                 "a deleted file must not appear even with the highest churn"
  end

  def test_rows_honor_the_limit
    churn = { "lib/a.rb" => 2, "lib/b.rb" => 1 }
    sizes = { "lib/a.rb" => 10, "lib/b.rb" => 10 }

    assert_equal 1, Hotspots.rows(churn: churn, sizes: sizes, fan_in: {}, limit: 1).size,
                 "scored rows through rows must cut to the requested limit"
  end

  # The count the report needs to disclose how many files its top-N view
  # leaves uncounted: every churned file that still has a size, matching
  # exactly what rows ranks (a vanished file drops out of both).
  def test_scored_total_counts_ranked_files_and_drops_vanished_ones
    churn = { "lib/a.rb" => 10, "lib/b.rb" => 2, "lib/gone.rb" => 99 }
    sizes = { "lib/a.rb" => 10, "lib/b.rb" => 500 }

    assert_equal 2, Hotspots.scored_total(churn: churn, sizes: sizes),
                 "scored_total counts only churned files that still have a size, matching the ranking"
  end

  # Rust carries its tests inline while Ruby's live in the excluded
  # test/ tree — sizing whole .rs files would skew the cross-language
  # churn × size ranking by the weight of their test tails.
  def test_impl_lines_strip_a_rust_test_tail_and_keep_ruby_whole
    rust = "pub fn shipped() {}\n#[cfg(test)]\nmod tests {\n    fn helper() {}\n}\n"
    ruby = "def shipped\nend\n"

    assert_equal 1, Hotspots.impl_lines("crates/kobako-codec/src/codec.rs", rust),
                 "a Rust file through impl_lines must count only the lines before its cfg(test) tail"
    assert_equal 2, Hotspots.impl_lines("lib/kobako/sandbox.rb", ruby),
                 "a Ruby file through impl_lines must count every line"
  end
end
