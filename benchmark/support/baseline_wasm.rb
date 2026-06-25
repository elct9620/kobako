# frozen_string_literal: true

require "tmpdir"

module Kobako
  module Bench
    # Resolves a +bench:confirm+ baseline reference to a Guest Binary on
    # disk: an explicit path, the GitHub release asset for +v<ref>+, or —
    # for releases predating asset uploads — the wasm packed inside the
    # published gem.
    module BaselineWasm
      module_function

      def resolve(ref)
        return ref if File.exist?(ref)

        fetch_release_asset(ref) || fetch_gem_wasm(ref) ||
          abort("bench:confirm: no Guest Binary for #{ref} — pass a wasm path, " \
                "or check `gh release view v#{ref}`.")
      end

      def fetch_release_asset(version)
        dir = Dir.mktmpdir("kobako-baseline")
        ok = system("gh", "release", "download", "v#{version}", "--pattern", "*.wasm", "--dir", dir,
                    out: File::NULL, err: File::NULL)
        Dir[File.join(dir, "*.wasm")].first if ok
      end

      def fetch_gem_wasm(version)
        dir = Dir.mktmpdir("kobako-baseline")
        fetched = system("gem", "fetch", "kobako", "-v", version, "--platform", "ruby",
                         chdir: dir, out: File::NULL, err: File::NULL)
        gem = fetched && Dir[File.join(dir, "*.gem")].first
        return nil unless gem && system("gem", "unpack", gem, "--target", dir, out: File::NULL, err: File::NULL)

        Dir[File.join(dir, "kobako-*", "data", "kobako.wasm")].first
      end
    end
  end
end
