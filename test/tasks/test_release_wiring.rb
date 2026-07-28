# frozen_string_literal: true

require "test_helper"

require_relative "../../tasks/support/release_wiring"

# Unit coverage for the readers behind gate:release:wiring
# (docs/releasing.md § Adding a crate to the linked group). Both seats the
# gate holds fail without a symptom: release-please treats a package absent
# from the manifest as never released, and rewrites a version only on a line
# carrying the annotation — reporting success either way.
class KobakoReleaseWiringTest < Minitest::Test
  Reader = KobakoReleaseWiring

  CONFIG = {
    "packages" => {
      "crates/kobako-transport" => {
        "component" => "kobako-transport",
        "extra-files" => [
          { "type" => "toml", "path" => "/Cargo.lock", "jsonpath" => "$.package[0].version" },
          { "type" => "generic", "path" => "/crates/kobako-transport/README.md" }
        ]
      },
      "crates/kobako-codec" => {
        "component" => "kobako-codec",
        "extra-files" => [{ "type" => "generic", "path" => "/crates/kobako-codec/README.md" }]
      }
    }
  }.freeze

  MANIFEST = { "crates/kobako-transport" => "0.12.0", "crates/kobako-codec" => "0.12.0" }.freeze

  # ---------- Manifest record ----------

  def test_a_package_the_manifest_omits_is_reported
    manifest = MANIFEST.except("crates/kobako-transport")

    assert_equal ["crates/kobako-transport"], Reader.unrecorded_packages(config: CONFIG, manifest: manifest),
                 "a package declared in the config but absent from the manifest must be reported, since " \
                 "release-please reads that absence as never released and never bumps it with the group"
  end

  def test_every_package_recorded_yields_no_violation
    assert_empty Reader.unrecorded_packages(config: CONFIG, manifest: MANIFEST),
                 "a config whose packages the manifest all record must pass the manifest half of the gate"
  end

  # ---------- Annotated version files ----------

  # The generic updater is the only one an annotation drives; the others
  # name their target with a jsonpath, so they carry no annotation and must
  # not be demanded one.
  def test_only_generic_extra_files_are_collected
    assert_equal [["crates/kobako-transport", "crates/kobako-transport/README.md"],
                  ["crates/kobako-codec", "crates/kobako-codec/README.md"]],
                 Reader.annotated_files(CONFIG),
                 "extra-file collection must return the generic entries alone, path-relative, since a " \
                 "jsonpath-driven entry needs no in-file annotation"
  end

  def test_a_file_without_the_annotation_is_reported
    files = [["crates/kobako-transport", "README.md", "kobako-transport = \"0.12.0\"\n"]]

    assert_equal ["README.md: no x-release-please-version annotation"],
                 Reader.annotation_violations(files: files, manifest: MANIFEST),
                 "a generic extra-file carrying no annotation must be reported, since the updater rewrites " \
                 "only an annotated line and reports success having changed nothing"
  end

  def test_a_declared_file_that_does_not_exist_is_reported_as_absent
    files = [["crates/kobako-transport", "README.md", nil]]

    assert_equal ["README.md: declared as a generic extra-file but absent"],
                 Reader.annotation_violations(files: files, manifest: MANIFEST),
                 "a generic extra-file naming a path that does not exist must be reported as absent rather " \
                 "than as unannotated, since the two are fixed differently"
  end

  def test_an_annotated_version_behind_the_manifest_is_reported
    files = [["crates/kobako-transport", "README.md", "kobako-transport = \"0.11.0\" # x-release-please-version\n"]]

    assert_equal ["README.md: annotates 0.11.0, manifest records crates/kobako-transport at 0.12.0"],
                 Reader.annotation_violations(files: files, manifest: MANIFEST),
                 "an annotated version disagreeing with the manifest must be reported as drift"
  end

  # A README pinning a sibling alongside itself gets every annotated line
  # rewritten to the releasing package's version, so the two are only
  # correct while they agree.
  def test_every_annotated_line_is_checked_not_only_the_first
    content = <<~MD
      kobako-mruby = "0.12.0" # x-release-please-version
      kobako-core = "0.11.0" # x-release-please-version
    MD
    files = [["crates/kobako-transport", "README.md", content]]

    assert_equal ["README.md: annotates 0.11.0, manifest records crates/kobako-transport at 0.12.0"],
                 Reader.annotation_violations(files: files, manifest: MANIFEST),
                 "a second annotated line behind the manifest must be reported, since the updater sets " \
                 "every annotated line to the releasing package's version"
  end

  def test_a_file_whose_package_the_manifest_omits_is_left_to_the_manifest_check
    files = [["crates/kobako-json", "README.md", "no annotation here\n"]]

    assert_empty Reader.annotation_violations(files: files, manifest: MANIFEST),
                 "a file whose package the manifest never records must not be double-reported here, since " \
                 "the manifest check already names that package"
  end

  def test_an_annotation_matching_the_manifest_yields_no_violation
    files = [["crates/kobako-transport", "README.md", "kobako-transport = \"0.12.0\" # x-release-please-version\n"]]

    assert_empty Reader.annotation_violations(files: files, manifest: MANIFEST),
                 "an annotated version matching what the manifest records must pass the annotation half"
  end
end

# The third seat +gate:release:wiring+ holds: release-please rewrites a
# version in +[dependencies]+ but walks no +[dev-dependencies]+ table, so a
# version named there stays at the last release while the crate beside it
# moves on — and the lockfile step is where that surfaces, one release later.
class KobakoReleaseWiringDevDependencyTest < Minitest::Test
  Reader = KobakoReleaseWiring

  def test_a_dev_dependency_naming_a_version_is_reported
    manifest = <<~TOML
      [dev-dependencies]
      kobako-codec = { path = "../kobako-codec", version = "0.12.0" }
    TOML

    assert_equal ["crates/kobako/Cargo.toml: dev-dependency kobako-codec names a version"],
                 Reader.pinned_dev_dependencies("crates/kobako/Cargo.toml", manifest),
                 "a path dev-dependency naming a version must be reported, since the release " \
                 "tooling rewrites no version in that table"
  end

  def test_a_path_only_dev_dependency_yields_no_violation
    manifest = <<~TOML
      [dev-dependencies]
      kobako-codec = { path = "../kobako-codec" }
    TOML

    assert_empty Reader.pinned_dev_dependencies("crates/kobako/Cargo.toml", manifest),
                 "a version-less path dev-dependency must pass: cargo strips it when packaging, " \
                 "so nothing needs rewriting"
  end

  def test_a_registry_dev_dependency_yields_no_violation
    manifest = <<~TOML
      [dev-dependencies]
      serde_json = "1.0"
    TOML

    assert_empty Reader.pinned_dev_dependencies("crates/kobako/Cargo.toml", manifest),
                 "a dev-dependency on a published crate must pass, since no release of ours moves it"
  end

  def test_a_version_outside_the_dev_dependency_table_is_left_alone
    manifest = <<~TOML
      [dependencies]
      kobako-codec = { path = "../kobako-codec", version = "0.12.0" }

      [features]
      default = []
    TOML

    assert_empty Reader.pinned_dev_dependencies("crates/kobako/Cargo.toml", manifest),
                 "a normal dependency must be left alone: the release tooling does rewrite that " \
                 "table, and cargo requires the version there"
  end
end
