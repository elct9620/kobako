# frozen_string_literal: true

require "fileutils"

module KobakoVendor
  # Unpacks a vendored tarball into +final_dir+, idempotent on a
  # +.kobako-version+ marker stamped inside the tree. One instance per
  # +(tarball, top_level_dir, final_dir, version)+ configuration; reuse is
  # not supported and not needed by +tasks/vendor.rake+.
  #
  # A version mismatch (toolchain bump) forces a clean re-extract, so the
  # unpacked tree never lags the pinned version. Public contract is the
  # single +#prepare+ entry point; the staging-directory step is internal.
  class Tarball
    # Marker stamped inside +final_dir+ after a successful unpack; a matching
    # value short-circuits +#prepare+, a mismatch forces re-extract.
    VERSION_MARKER = ".kobako-version"

    def initialize(tarball:, top_level_dir:, final_dir:, version:)
      @tarball = tarball
      @top_level_dir = top_level_dir
      @final_dir = final_dir
      @version = version
    end

    # Extract the tarball into a staging sibling of +final_dir+, then
    # atomically move the +top_level_dir+ subtree into place and stamp the
    # version marker. A no-op when the stamped version already matches.
    # Raises if the tarball does not contain the expected +top_level_dir+ root.
    def prepare
      return if installed_version == @version

      staging = extract_to_staging
      src = File.join(staging, @top_level_dir)
      raise "[vendor] expected #{src} after extracting #{@tarball}, missing" unless File.directory?(src)

      FileUtils.rm_rf(@final_dir)
      FileUtils.mkdir_p(File.dirname(@final_dir))
      FileUtils.mv(src, @final_dir)
      File.write(File.join(@final_dir, VERSION_MARKER), "#{@version}\n")
      FileUtils.rm_rf(staging)
    end

    private

    # Version recorded by the last successful unpack, or +nil+ when the tree
    # is absent or predates version stamping (forcing a re-extract).
    def installed_version
      marker = File.join(@final_dir, VERSION_MARKER)
      File.read(marker).strip if File.exist?(marker)
    end

    def extract_to_staging
      staging = "#{@final_dir}.staging"
      FileUtils.rm_rf(staging)
      FileUtils.mkdir_p(staging)
      system("tar", "-xzf", @tarball, "-C", staging, exception: true)
      staging
    end
  end
end
