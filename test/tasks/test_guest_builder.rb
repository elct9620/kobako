# frozen_string_literal: true

require "test_helper"
require "tmpdir"

require_relative "../../tasks/support/wasm"

# Unit coverage for when a Guest Binary build stages libmruby.a first. CI
# restores the Guest Binaries one job built into jobs holding no
# archive, where staging one would make them look stale and rebuild.
class KobakoGuestBuilderTest < Minitest::Test
  # A builder that sees libmruby.a on disk as +archive+ says.
  class Builder < KobakoWasm::GuestBuilder
    def initialize(archive:, **)
      super(**)
      @archive = archive
    end

    private

    def archive_on_disk? = @archive
  end

  def test_a_current_guest_without_the_archive_needs_none
    in_built_guest do |output|
      refute Builder.new(archive: false, output: output).needs_archive?,
             "a Guest Binary newer than its sources, with no libmruby.a on disk, through " \
             "GuestBuilder#needs_archive? must not need the archive"
    end
  end

  def test_a_missing_guest_needs_the_archive
    Dir.mktmpdir do |dir|
      assert Builder.new(archive: false, output: File.join(dir, "kobako.wasm")).needs_archive?,
             "a Guest Binary not yet built through GuestBuilder#needs_archive? must need the archive"
    end
  end

  def test_an_archive_on_disk_is_always_staged_again
    in_built_guest do |output|
      assert Builder.new(archive: true, output: output).needs_archive?,
             "a current Guest Binary with libmruby.a on disk through GuestBuilder#needs_archive? " \
             "must need the archive, which staging may rebuild"
    end
  end

  def test_the_sources_hold_the_cross_workspace_crates_and_no_archive
    sources = KobakoWasm::GuestBuilder.sources

    assert_equal [true, true, false],
                 [sources.any? { |path| path.include?("/crates/kobako-transport/src/") },
                  sources.include?(File.join(KobakoWasm::WASM_WORKSPACE_DIR, "kobako-baker", "Cargo.lock")),
                  sources.include?(KobakoWasm::LIBMRUBY_PATH)],
                 "GuestBuilder.sources must list kobako-transport's sources and the baker lock, " \
                 "and leave libmruby.a out"
  end

  private

  # A Guest Binary written now, so it is newer than every source.
  def in_built_guest
    Dir.mktmpdir do |dir|
      output = File.join(dir, "kobako.wasm")
      File.write(output, "")
      yield output
    end
  end
end
