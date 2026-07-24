# frozen_string_literal: true

require "test_helper"

require_relative "../../tasks/support/gvl_isolation"

# Unit coverage for the reader behind gate:gvl:isolation (B-64): the gate
# fails if the wasmtime driver's manifest names magnus, because the
# GVL-released span calls into that driver and a magnus dependency would put
# a Ruby VALUE in its reach. The reader lists the magnus-naming lines,
# skipping comments so a mention in prose does not read as a dependency.
class KobakoGvlIsolationTest < Minitest::Test
  Reader = KobakoGvlIsolation

  def test_reports_a_magnus_dependency_line
    manifest = <<~TOML
      [dependencies]
      magnus = { version = "0.8.2" }
      kobako-runtime = { path = "../kobako-runtime" }
    TOML

    assert_equal ['magnus = { version = "0.8.2" }'], Reader.magnus_mentions(manifest),
                 "a manifest that declares magnus must surface the dependency line (B-64)"
  end

  def test_ignores_magnus_named_only_in_a_comment
    manifest = <<~TOML
      [dependencies]
      # wasmtime alone here; magnus never enters this driver's graph
      wasmtime = "38"
    TOML

    assert_empty Reader.magnus_mentions(manifest),
                 "magnus named only in a comment is prose, not a dependency (B-64)"
  end

  def test_a_magnus_free_manifest_yields_no_mentions
    manifest = <<~TOML
      [dependencies]
      wasmtime = "38"
      kobako-codec = { path = "../kobako-codec" }
    TOML

    assert_empty Reader.magnus_mentions(manifest),
                 "a driver manifest that names no magnus must pass the isolation gate (B-64)"
  end
end
