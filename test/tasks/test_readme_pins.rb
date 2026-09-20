# frozen_string_literal: true

require "test_helper"

require_relative "../../tasks/support/readme_pins"

# Unit coverage for the README install-snippet comparator: a third-party pin
# is held to the manifest's series, a shortened series still matches, the
# release tooling's own pins are exempt, and prose outside a TOML fence is
# not read as a pin.
class KobakoReadmePinsTest < Minitest::Test
  ReadmePins = KobakoReadmePins

  MANIFEST = <<~TOML
    [package]
    name = "kobako-regexp"
    version = "0.16.0"

    [dependencies]
    kobako-transport = { path = "../../crates/kobako-transport", version = "0.16.0" }
    beni = "0.18.0"
    fancy-regex = "0.16"
  TOML

  def readme(snippet)
    <<~MARKDOWN
      # Title

      ```toml
      #{snippet}
      ```
    MARKDOWN
  end

  def test_a_pin_naming_the_manifest_series_is_in_sync
    assert_empty ReadmePins.drift(readme: readme('beni = "0.18"'), manifest: MANIFEST),
                 "a snippet shortening the manifest's 0.18.0 to 0.18 must read as the same series"
  end

  def test_an_exact_pin_is_in_sync
    assert_empty ReadmePins.drift(readme: readme('beni = "0.18.0"'), manifest: MANIFEST),
                 "a snippet repeating the manifest version exactly must read as in sync"
  end

  def test_a_pin_naming_an_older_series_drifts
    assert_equal [["beni", "0.14", "0.18.0"]],
                 ReadmePins.drift(readme: readme('beni = "0.14"'), manifest: MANIFEST),
                 "a snippet naming a series the manifest does not depend on must be reported with both versions"
  end

  def test_a_release_tooling_pin_is_exempt
    snippet = 'kobako-transport = "0.14.0" # x-release-please-version'

    assert_empty ReadmePins.drift(readme: readme(snippet), manifest: MANIFEST),
                 "a pin the release tooling moves must be left to it, whatever the manifest carries"
  end

  def test_a_dependency_the_manifest_lacks_is_left_alone
    assert_empty ReadmePins.drift(readme: readme('serde = "1.0"'), manifest: MANIFEST),
                 "a snippet line naming a dependency the consumer adds themselves must not be held to this manifest"
  end

  def test_a_version_outside_a_toml_fence_is_not_a_pin
    prose = "Install `beni = \"0.14\"` alongside it.\n\n```rust\nlet beni = \"0.14\";\n```\n"

    assert_empty ReadmePins.drift(readme: prose, manifest: MANIFEST),
                 "a version named in prose or a Rust fence must not be read as an install pin"
  end
end
