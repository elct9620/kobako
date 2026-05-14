# frozen_string_literal: true

module KobakoVendor
  # Declarative value object describing a tarball-style vendored
  # toolchain. Captures the +(remote, cache, unpacked)+ triple plus
  # the integrity-check key, and exposes the three pipeline stages
  # (+#fetch+, +#verify+, +#install+) that +tasks/vendor.rake+ wires
  # into +file+ / +task+ declarations.
  #
  # Adding a new tarball-based vendor artifact is a single Toolchain
  # constant in +KobakoVendor+; the rake DSL loop in +vendor.rake+
  # picks it up automatically. Artifacts whose pipeline does not match
  # this shape (e.g. +onigmo-build-aux+, which is a set of plain files,
  # not a tarball) stay declared by hand alongside the toolchain loop.
  #
  # Fields:
  #
  #   * +name+             — display name and base for the +setup:<name>+
  #                          task identifier (dashes become underscores).
  #   * +version_label+    — human-readable version string printed in the
  #                          download log; opaque to the pipeline itself.
  #   * +base_url+         — remote URL prefix; resolved through
  #                          +KobakoVendor.base_url_for+ so test fixtures
  #                          can override via +KOBAKO_VENDOR_BASE_URL+.
  #   * +tarball_name+     — filename joined to both +base_url+ (download)
  #                          and +CACHE_DIR+ (cache location).
  #   * +unpacked_top_dir+ — the single top-level directory produced when
  #                          the tarball is extracted.
  #   * +final_dir+        — destination under +VENDOR_DIR+ where the
  #                          unpacked tree is moved.
  #   * +sentinel+         — relative path inside +final_dir+ whose
  #                          presence marks the install as complete and
  #                          short-circuits +#install+.
  #   * +sha_key+          — symbol used by +KobakoVendor.expected_sha256+
  #                          to look up the +KOBAKO_VENDOR_<KEY>_SHA256+
  #                          environment variable.
  Toolchain = Data.define(
    :name, :version_label, :base_url, :tarball_name,
    :unpacked_top_dir, :final_dir, :sentinel, :sha_key
  ) do
    # Symbol used to identify the +setup:<task_name>+ rake task. Dashes
    # in +name+ are not valid in rake task identifiers, so we map them
    # to underscores at this single seam.
    def task_name
      name.tr("-", "_").to_sym
    end

    # Resolved download URL. Honours the +KOBAKO_VENDOR_BASE_URL+ test
    # fixture override at call time (not at constant-load time), so a
    # test can flip the env var after the Toolchain constant is frozen.
    def url
      "#{KobakoVendor.base_url_for(base_url)}/#{tarball_name}"
    end

    # Local cache path for the downloaded tarball. Lives under
    # +KobakoVendor::CACHE_DIR+ regardless of +VENDOR_DIR+ overrides
    # (the cache moves with the vendor tree).
    def tarball_path
      File.join(KobakoVendor::CACHE_DIR, tarball_name)
    end

    # Download the tarball into +tarball_path+ and verify its SHA256.
    # Intended as the body of the +file tarball_path+ rake task; the
    # task's mtime-based caching avoids re-downloading on a cache hit.
    def fetch
      puts "[vendor] downloading #{name} #{version_label} from #{url}"
      Downloader.new(url, tarball_path).download
      verify
    end

    # Recompute the cached tarball's SHA256 and check it against the
    # expected hash (or pin via TOFU sidecar). Idempotent — safe to
    # call from both +file+ and +setup+ task bodies when the latter
    # depends on the former.
    def verify
      Checksum.new(tarball_path, KobakoVendor.expected_sha256(sha_key)).verify_or_pin
    end

    # Verify the cached tarball, then unpack it into +final_dir+ via
    # +Tarball#prepare+. A no-op when +sentinel+ is already present
    # under +final_dir+.
    def install
      verify
      Tarball.new(
        tarball: tarball_path,
        top_level_dir: unpacked_top_dir,
        final_dir: final_dir,
        sentinel: sentinel
      ).prepare
      puts "[vendor] #{name} ready at #{final_dir}"
    end
  end
end
