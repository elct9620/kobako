# frozen_string_literal: true

require "test_helper"

require_relative "../../tasks/support/wasm"

# Unit coverage for the sources a Guest Binary is judged current by. CI
# restores the Guest Binaries one job built into jobs that stage the mruby
# archive afresh, so the archive counts through what stages it, never
# through its own mtime.
class KobakoGuestBuilderTest < Minitest::Test
  def test_the_sources_name_the_guest_crates_and_what_stages_the_archive
    pinned = %w[build_config/wasi.rb Gemfile.lock rust-toolchain.toml].map { |path| File.join(KobakoWasm::ROOT, path) }

    assert_equal [true, []],
                 [sources.any? { |path| path.include?("/crates/kobako-transport/src/") }, pinned - sources],
                 "GuestBuilder.sources must list kobako-transport's sources, the build config, " \
                 "Gemfile.lock, and rust-toolchain.toml"
  end

  def test_the_sources_leave_the_archive_out
    refute_includes sources, KobakoWasm::LIBMRUBY_PATH,
                    "GuestBuilder.sources must leave libmruby.a out, so staging it keeps a Guest Binary current"
  end

  private

  def sources = KobakoWasm::GuestBuilder.sources
end
