# frozen_string_literal: true

# Vendor toolchain rake task
# ==========================
#
# Fetches and unpacks the build-time toolchain into `vendor/`. The
# tarball-based artifacts (wasi-sdk, mruby, mruby-onig-regexp) are
# declared as +KobakoVendor::Toolchain+ values in
# +tasks/support/kobako_vendor.rb+; this file iterates over
# +KobakoVendor::TARBALL_TOOLCHAINS+ to wire one +file+ task and one
# +setup:<name>+ task per artifact. Adding a new tarball artifact is a
# single +Toolchain.new(...)+ entry — no rake DSL surgery required.
#
# The fourth artifact (+onigmo-build-aux+: a pair of plain files
# overwriting Onigmo's pre-wasm config scripts, same pattern CRuby's
# wasm build uses) does not fit the tarball pipeline and stays declared
# by hand below the loop.
#
# Idempotency: every step is a +file+ task targeting a sentinel path
# inside the cache or unpacked tree; re-runs short-circuit when the
# sentinel exists.
#
# Honors +KOBAKO_VENDOR_BASE_URL+ to point downloads at a local fixture
# during tests, and +KOBAKO_VENDOR_DIR+ to relocate the entire vendor
# tree (also test-only).

require_relative "support/kobako_vendor"

namespace :vendor do
  KobakoVendor::TARBALL_TOOLCHAINS.each do |toolchain|
    file toolchain.tarball_path do
      toolchain.fetch
    end
  end

  KobakoVendor::CONFIG_AUX_FILES.each do |filename|
    commit = KobakoVendor::CONFIG_AUX_COMMIT
    cache_path = File.join(KobakoVendor::CACHE_DIR, "#{filename}.#{commit}")
    file cache_path do |t|
      url = "https://git.savannah.gnu.org/cgit/config.git/plain/#{filename}?id=#{commit}"
      puts "[vendor] downloading #{filename}@#{commit[0, 8]} from #{url}"
      KobakoVendor::Downloader.new(url, t.name).download
      sha_key = :"CONFIG_#{filename.split(".").last.upcase}"
      KobakoVendor::Checksum.new(t.name, KobakoVendor.expected_sha256(sha_key)).verify_or_pin
    end
  end

  namespace :setup do
    KobakoVendor::TARBALL_TOOLCHAINS.each do |toolchain|
      desc "Download and unpack #{toolchain.name} #{toolchain.version_label} into #{toolchain.final_dir}"
      task toolchain.task_name => toolchain.tarball_path do
        toolchain.install
      end
    end

    desc "Download GNU config.sub / config.guess into vendor/onigmo-build-aux/"
    aux_cache_paths = KobakoVendor::CONFIG_AUX_FILES.map do |f|
      File.join(KobakoVendor::CACHE_DIR, "#{f}.#{KobakoVendor::CONFIG_AUX_COMMIT}")
    end
    task onigmo_build_aux: aux_cache_paths do
      FileUtils.mkdir_p(KobakoVendor::CONFIG_AUX_FINAL)
      KobakoVendor::CONFIG_AUX_FILES.each_with_index do |filename, idx|
        dst = File.join(KobakoVendor::CONFIG_AUX_FINAL, filename)
        FileUtils.cp(aux_cache_paths[idx], dst)
        File.chmod(0o755, dst)
      end
      puts "[vendor] onigmo-build-aux ready at #{KobakoVendor::CONFIG_AUX_FINAL}"
    end
  end

  desc "Fetch and unpack all build-time vendor toolchains (wasi-sdk + mruby + mruby-onig-regexp + onigmo-build-aux)"
  task setup: KobakoVendor::TARBALL_TOOLCHAINS.map { |t| "setup:#{t.task_name}" } + ["setup:onigmo_build_aux"]

  desc "Remove unpacked vendor toolchains (keeps cached tarballs)"
  task :clean do
    KobakoVendor::TARBALL_TOOLCHAINS.each { |t| FileUtils.rm_rf(t.final_dir) }
    FileUtils.rm_rf(KobakoVendor::CONFIG_AUX_FINAL)
  end

  desc "Remove vendor/ entirely (unpacked trees and cached tarballs)"
  task :clobber do
    FileUtils.rm_rf(KobakoVendor::VENDOR_DIR)
  end
end
