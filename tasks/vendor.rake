# frozen_string_literal: true

# Vendor toolchain rake task
# ==========================
#
# Fetches and unpacks the build-time toolchain into `vendor/`:
#
#   * wasi-sdk          — Clang + wasi-sysroot + libsetjmp, used to
#                         cross-compile both mruby (Stage B) and the wasm
#                         crate (Stage C). Version must be >= 26 (see
#                         SPEC.md "Implementation Standards" §setjmp/longjmp).
#   * mruby             — pinned release tarball used as the guest VM source.
#   * mruby-onig-regexp — pinned-commit tarball of the third-party regex
#                         mrbgem (Onigmo engine, vendored inside the gem
#                         tarball as `onigmo-6.2.0.tar.gz`). Loaded by
#                         `build_config/wasi.rb` via `conf.gem <path>`.
#   * onigmo-build-aux  — pinned-commit GNU config.sub / config.guess
#                         (single-file shell scripts) that overwrite the
#                         pre-wasm copies shipped inside Onigmo 6.2.0.
#                         Same pattern CRuby's wasm build uses; the actual
#                         overwrite happens in
#                         `KobakoBuildConfig.pre_extract_and_patch_onigmo!`
#                         (build_config/wasi.rb), called from the
#                         CrossBuild block right after `conf.gem` loads
#                         the mrbgem.
#
# Versions are pinned as constants in `KobakoVendor` (see
# tasks/support/kobako_vendor.rb). Bumping a version is the entire
# upgrade workflow; no git submodule pointer dance.
#
# Idempotency: every step is a `file` task that targets a sentinel path inside
# the unpacked tree. Re-runs short-circuit when the sentinel exists.
#
# Honors `KOBAKO_VENDOR_BASE_URL` to point downloads at a local fixture during
# tests, and `KOBAKO_VENDOR_DIR` to relocate the entire vendor tree (also
# test-only).

require_relative "support/kobako_vendor"

namespace :vendor do
  # File-level cache: tarballs land in vendor/.cache/.
  file KobakoVendor::WASI_TARBALL_PATH do |t|
    url = "#{KobakoVendor.base_url_for(KobakoVendor::DEFAULT_WASI_SDK_BASE)}/" \
          "#{KobakoVendor::WASI_SDK_TARBALL_NAME}"
    puts "[vendor] downloading wasi-sdk #{KobakoVendor::WASI_SDK_FULL_VERSION} " \
         "(#{KobakoVendor::WASI_SDK_PLATFORM}) from #{url}"
    KobakoVendor::Downloader.new(url, t.name).download
    KobakoVendor::Checksum.new(t.name, KobakoVendor.expected_sha256(:WASI_SDK)).verify_or_pin
  end

  file KobakoVendor::MRUBY_TARBALL_PATH do |t|
    url = "#{KobakoVendor.base_url_for(KobakoVendor::DEFAULT_MRUBY_BASE)}/" \
          "#{KobakoVendor::MRUBY_TARBALL_NAME}"
    puts "[vendor] downloading mruby #{KobakoVendor::MRUBY_VERSION} from #{url}"
    KobakoVendor::Downloader.new(url, t.name).download
    KobakoVendor::Checksum.new(t.name, KobakoVendor.expected_sha256(:MRUBY)).verify_or_pin
  end

  file KobakoVendor::MRUBY_ONIG_REGEXP_TARBALL_PATH do |t|
    url = "#{KobakoVendor.base_url_for(KobakoVendor::DEFAULT_MRUBY_ONIG_REGEXP_BASE)}/" \
          "#{KobakoVendor::MRUBY_ONIG_REGEXP_TARBALL_NAME}"
    puts "[vendor] downloading mruby-onig-regexp #{KobakoVendor::MRUBY_ONIG_REGEXP_COMMIT[0, 8]} from #{url}"
    KobakoVendor::Downloader.new(url, t.name).download
    KobakoVendor::Checksum.new(t.name, KobakoVendor.expected_sha256(:MRUBY_ONIG_REGEXP)).verify_or_pin
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
    desc "Download and unpack wasi-sdk #{KobakoVendor::WASI_SDK_FULL_VERSION} into vendor/wasi-sdk/"
    task wasi_sdk: KobakoVendor::WASI_TARBALL_PATH do
      KobakoVendor::Checksum.new(KobakoVendor::WASI_TARBALL_PATH, KobakoVendor.expected_sha256(:WASI_SDK)).verify_or_pin
      KobakoVendor.prepare_unpacked(
        tarball: KobakoVendor::WASI_TARBALL_PATH,
        top_level_dir: KobakoVendor::WASI_SDK_UNPACKED_DIR,
        final_dir: KobakoVendor::WASI_SDK_FINAL,
        sentinel: KobakoVendor::WASI_SDK_SENTINEL
      )
      puts "[vendor] wasi-sdk ready at #{KobakoVendor::WASI_SDK_FINAL}"
    end

    desc "Download and unpack mruby #{KobakoVendor::MRUBY_VERSION} into vendor/mruby/"
    task mruby: KobakoVendor::MRUBY_TARBALL_PATH do
      KobakoVendor::Checksum.new(KobakoVendor::MRUBY_TARBALL_PATH, KobakoVendor.expected_sha256(:MRUBY)).verify_or_pin
      KobakoVendor.prepare_unpacked(
        tarball: KobakoVendor::MRUBY_TARBALL_PATH,
        top_level_dir: KobakoVendor::MRUBY_UNPACKED_DIR,
        final_dir: KobakoVendor::MRUBY_FINAL,
        sentinel: KobakoVendor::MRUBY_SENTINEL
      )
      puts "[vendor] mruby ready at #{KobakoVendor::MRUBY_FINAL}"
    end

    desc "Download and unpack mruby-onig-regexp into vendor/mruby-onig-regexp/"
    task mruby_onig_regexp: KobakoVendor::MRUBY_ONIG_REGEXP_TARBALL_PATH do
      KobakoVendor::Checksum.new(KobakoVendor::MRUBY_ONIG_REGEXP_TARBALL_PATH,
                                 KobakoVendor.expected_sha256(:MRUBY_ONIG_REGEXP)).verify_or_pin
      KobakoVendor.prepare_unpacked(
        tarball: KobakoVendor::MRUBY_ONIG_REGEXP_TARBALL_PATH,
        top_level_dir: KobakoVendor::MRUBY_ONIG_REGEXP_UNPACKED_DIR,
        final_dir: KobakoVendor::MRUBY_ONIG_REGEXP_FINAL,
        sentinel: KobakoVendor::MRUBY_ONIG_REGEXP_SENTINEL
      )
      puts "[vendor] mruby-onig-regexp ready at #{KobakoVendor::MRUBY_ONIG_REGEXP_FINAL}"
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
  task setup: ["setup:wasi_sdk", "setup:mruby", "setup:mruby_onig_regexp", "setup:onigmo_build_aux"]

  desc "Remove unpacked vendor toolchains (keeps cached tarballs)"
  task :clean do
    FileUtils.rm_rf(KobakoVendor::WASI_SDK_FINAL)
    FileUtils.rm_rf(KobakoVendor::MRUBY_FINAL)
    FileUtils.rm_rf(KobakoVendor::MRUBY_ONIG_REGEXP_FINAL)
    FileUtils.rm_rf(KobakoVendor::CONFIG_AUX_FINAL)
  end

  desc "Remove vendor/ entirely (unpacked trees and cached tarballs)"
  task :clobber do
    FileUtils.rm_rf(KobakoVendor::VENDOR_DIR)
  end
end
