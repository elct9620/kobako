# frozen_string_literal: true

require "fileutils"

module KobakoVendor
  # Unpacks a vendored tarball into +final_dir+, idempotent on the
  # +sentinel+ file. One instance per +(tarball, top_level_dir, final_dir,
  # sentinel)+ configuration; reuse is not supported and not needed by
  # +tasks/vendor.rake+.
  #
  # Public contract is the single +#prepare+ entry point; the staging-
  # directory step is internal.
  class Tarball
    def initialize(tarball:, top_level_dir:, final_dir:, sentinel:)
      @tarball = tarball
      @top_level_dir = top_level_dir
      @final_dir = final_dir
      @sentinel = sentinel
    end

    # Extract the tarball into a staging sibling of +final_dir+, then
    # atomically move the +top_level_dir+ subtree into place. A no-op
    # when +sentinel+ already exists under +final_dir+. Raises if the
    # tarball does not contain the expected +top_level_dir+ root.
    def prepare
      return if File.exist?(File.join(@final_dir, @sentinel))

      staging = extract_to_staging
      src = File.join(staging, @top_level_dir)
      raise "expected #{src} after extracting #{@tarball}, missing" unless File.directory?(src)

      FileUtils.rm_rf(@final_dir)
      FileUtils.mkdir_p(File.dirname(@final_dir))
      FileUtils.mv(src, @final_dir)
      FileUtils.rm_rf(staging)
    end

    private

    def extract_to_staging
      staging = "#{@final_dir}.staging"
      FileUtils.rm_rf(staging)
      FileUtils.mkdir_p(staging)
      system("tar", "-xzf", @tarball, "-C", staging, exception: true)
      staging
    end
  end
end
