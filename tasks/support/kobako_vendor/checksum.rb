# frozen_string_literal: true

require "digest"

module KobakoVendor
  # SHA256 verification for vendored tarballs. Operates in two modes:
  # against an explicit expected hash (e.g. from CI env var) or trust-on-
  # first-use against a +.sha256+ sidecar file kept next to the tarball
  # in +KobakoVendor::CACHE_DIR+.
  module Checksum
    def self.sha256_of(path)
      Digest::SHA256.file(path).hexdigest
    end

    # Verify the tarball against expected_sha (if non-empty) or TOFU-pin it.
    # Raises on mismatch.
    def self.verify_or_pin(path, expected_sha)
      actual = sha256_of(path)
      sidecar = "#{path}.sha256"

      if expected_sha && !expected_sha.empty?
        verify_against_expected(path, actual, expected_sha, sidecar)
      else
        verify_or_pin_sidecar(path, actual, sidecar)
      end

      actual
    end

    def self.verify_against_expected(path, actual, expected_sha, sidecar)
      unless actual == expected_sha
        raise "checksum mismatch for #{File.basename(path)}: " \
              "expected #{expected_sha}, got #{actual}"
      end
      File.write(sidecar, "#{actual}\n")
    end

    def self.verify_or_pin_sidecar(path, actual, sidecar)
      if File.exist?(sidecar)
        pinned = File.read(sidecar).strip
        return if actual == pinned

        raise "checksum drift for #{File.basename(path)}: " \
              "pinned #{pinned}, got #{actual}"
      end
      File.write(sidecar, "#{actual}\n")
    end
  end
end
