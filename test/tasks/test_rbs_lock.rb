# frozen_string_literal: true

require "test_helper"

require_relative "../../tasks/support/rbs_lock"

# Unit coverage for the RBS collection-lock comparator: only gem-sourced
# pins are held to Gemfile.lock, git-collection and stdlib sources are
# exempt, and a mismatch or an absent gem surfaces as drift.
class KobakoRbsLockTest < Minitest::Test
  RbsLock = KobakoRbsLock

  # A collection lock mixing every source kind: one gem-sourced pin, one
  # git-collection pin, and one stdlib pin.
  COLLECTION = <<~YAML
    ---
    path: ".gem_rbs_collection"
    gems:
    - name: beni
      version: 0.8.1
      source:
        type: rubygems
    - name: rubocop
      version: '1.57'
      source:
        type: git
        name: ruby/gem_rbs_collection
    - name: json
      version: '0'
      source:
        type: stdlib
    gemfile_lock_path: Gemfile.lock
  YAML

  GEMFILE_LOCK = <<~LOCK
    GEM
      remote: https://rubygems.org/
      specs:
        beni (0.8.1)
        rubocop (1.57.2)
        json (2.7.1)
  LOCK

  # The gem-sourced beni pin equals Gemfile.lock, while the git-collection
  # (rubocop 1.57 vs 1.57.2) and stdlib (json 0 vs 2.7.1) pins deliberately
  # differ — only the former is held to Gemfile.lock, so the whole lock
  # reports no drift.
  def test_no_drift_when_gem_sourced_pins_match_regardless_of_other_sources
    assert_empty RbsLock.drift(collection_yaml: COLLECTION, gemfile_lock: GEMFILE_LOCK),
                 "a matching gem-sourced pin is clean and git / stdlib version differences are exempt"
  end

  def test_gem_sourced_version_mismatch_is_drift
    stale = COLLECTION.sub("version: 0.8.1", "version: 0.8.0")

    assert_equal [["beni", "0.8.0", "0.8.1"]],
                 RbsLock.drift(collection_yaml: stale, gemfile_lock: GEMFILE_LOCK),
                 "a gem-sourced pin behind Gemfile.lock must surface as drift naming both versions"
  end

  def test_gem_sourced_pin_absent_from_gemfile_lock_is_drift
    without_beni = GEMFILE_LOCK.sub("    beni (0.8.1)\n", "")

    assert_equal [["beni", "0.8.1", nil]],
                 RbsLock.drift(collection_yaml: COLLECTION, gemfile_lock: without_beni),
                 "a gem-sourced pin whose gem is absent from Gemfile.lock must surface as drift"
  end
end
