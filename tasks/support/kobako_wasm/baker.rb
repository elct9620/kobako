# frozen_string_literal: true

require "fileutils"
require "tmpdir"

module KobakoWasm
  # Stage C bake step: runs the +kobako-baker+ tool over a linked Guest
  # Binary so the shipped artifact embeds the canonical boot state.
  # The bake runs twice and the outputs must be
  # byte-identical — the F-10 reproducibility gate; a divergence aborts
  # the build instead of shipping a nondeterministic image.
  class Baker
    BAKER_DIR      = File.join(WASM_WORKSPACE_DIR, "kobako-baker").freeze
    BAKER_MANIFEST = File.join(BAKER_DIR, "Cargo.toml").freeze
    BAKER_BIN      = File.join(BAKER_DIR, "target", "release", "kobako-baker").freeze

    # Bake +input+ into +output+, building the baker tool on demand.
    def bake(input, output)
      ensure_baker_built
      Dir.mktmpdir("kobako-bake") do |dir|
        FileUtils.cp(reproducible_bake(input, dir), output)
      end
      puts "[wasm:build] canonical boot state baked into #{output} (#{File.size(output)} bytes)"
    end

    private

    # Run the bake twice into +dir+ and return the artifact path only
    # when both runs agree byte-for-byte — the F-10 gate.
    def reproducible_bake(input, dir)
      first  = File.join(dir, "bake-1.wasm")
      second = File.join(dir, "bake-2.wasm")
      run_baker(input, first)
      run_baker(input, second)
      unless FileUtils.identical?(first, second)
        raise "[wasm:build] bake is not reproducible — two runs over #{input} diverged (F-10)"
      end

      first
    end

    def ensure_baker_built
      args = ["cargo", "build", "--release", "--manifest-path", BAKER_MANIFEST]
      puts "[wasm:build] ==> #{args.join(" ")}"
      raise "[wasm:build] kobako-baker build failed" unless system(*args)
      raise "[wasm:build] kobako-baker built but #{BAKER_BIN} is missing" unless File.exist?(BAKER_BIN)
    end

    def run_baker(input, output)
      return if system(BAKER_BIN, input, output)

      raise "[wasm:build] kobako-baker failed on #{input}"
    end
  end
end
