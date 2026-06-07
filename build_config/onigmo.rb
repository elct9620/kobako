# frozen_string_literal: true

require "digest"
require "fileutils"
require "net/http" # eager — TRANSIENT_ERRORS names Net::* at class-eval
require "open-uri"

module KobakoBuildConfig
  # Onigmo build preparation for the mruby-onig-regexp mrbgem: pre-extract
  # the bundled Onigmo source into the build tree, install wasm-aware GNU
  # config aux scripts, and patch the st_foreach callback arity. Required
  # by +build_config/wasi.rb+ after the core constants are defined; the
  # CrossBuild block calls +pre_extract_and_patch!+ once per build.
  module Onigmo
    # mruby-onig-regexp is fetched by mruby's own build system via
    # `conf.gem github:` in the CrossBuild block; `checksum_hash` pins a
    # content-addressed detached checkout, recorded in
    # `build_config/wasi.rb.lock`. Same strict-allowlist contract as
    # `MRBGEM_ALLOWLIST` but a separate surface: the gem pulls in a
    # native C dependency too, so the attack surface widens beyond the
    # gem's Ruby + glue C. Bumping the pin is a wire- / security-
    # review-bearing change.
    #
    # Onigmo is a guest-side compute capability; Regexp objects do NOT
    # cross the host↔guest wire (no SPEC.md wire codec change). The
    # Onigmo source bundled by the gem is frozen at 6.2.0 (2019) and
    # carries known CVEs covering ReDoS, OOB reads, and OOB writes;
    # upgrading requires forking the gem because the version is
    # hard-coded in its mrbgem.rake. The wasm sandbox isolates the host
    # from any guest-side crash, but a malicious / malformed pattern
    # can still corrupt guest state — host-side Sandbox limits (fuel,
    # memory) bound compute exhaustion but cannot bound engine-internal
    # memory-safety bugs.
    GEM_COMMIT = "c97d7c1e7073bc5558986da4e2d07171f0761cc8"

    # Onigmo 6.2.0 ships pre-wasm `config.sub` / `config.guess` that
    # reject `wasm32-wasi` host triples. mruby-onig-regexp's
    # `mrbgem.rake` extracts Onigmo into
    # `build/wasi/mrbgems/mruby-onig-regexp/onigmo-6.2.0/` only when its
    # +file header+ rake task fires, but the same +file+ rule is
    # idempotent (it skips when the sentinel exists). We pre-extract the
    # tarball and overwrite the aux scripts here so the rule sees the
    # sentinel and falls through to +./configure+, which then picks up
    # the modern wasm-aware aux scripts. Hooking the rake task graph
    # directly is not viable: mruby's build system registers gem file
    # tasks later in a separate pass than the build_config DSL.
    RELATIVE_BUILD_DIR = "vendor/mruby/build/#{MRUBY_BUILD_NAME}/mrbgems/mruby-onig-regexp".freeze
    RELATIVE_TARBALL   =
      "vendor/mruby/build/repos/#{MRUBY_BUILD_NAME}/mruby-onig-regexp/onigmo-6.2.0.tar.gz".freeze
    VERSION_DIR        = "onigmo-6.2.0"

    # GNU config.sub / config.guess replacements (same source CRuby's
    # wasm build uses, see ruby/ruby +wasm/README.md+). Fetched from GNU
    # savannah's config.git pinned to a commit (cgit serves
    # +plain/<path>?id=<sha>+ byte-stable) and verified against the
    # hard-pinned SHA256s below; cached under
    # +vendor/.cache/<name>.<commit>+ so one download serves every
    # re-extraction.
    CONFIG_AUX_COMMIT = "a2287c3041a3f2a204eb942e09c015eab00dc7dd"
    CONFIG_AUX_SHA256 = {
      "config.sub" => "26b852f75a637448360a956931439f7e818bf63150eaadb9b85484347628d1fd",
      "config.guess" => "50205cf3ec5c7615b17f937a0a57babf4ec5cd0aade3d7b3cccbe5f1bf91a7ef"
    }.freeze

    # Onigmo 6.2.0's `st_general_foreach` invokes its name-table callbacks
    # with four arguments while the four `regparse.c` callbacks are declared
    # with three; the `ANYARGS` cast hides the mismatch. Native targets ignore
    # the extra argument, but wasm32 type-checks every `call_indirect` and
    # hard-traps the moment a named-capture pattern compiles and walks its
    # name table. Align each callback to the call site by appending the
    # ignored fourth parameter (the same migration CRuby applied for wasm /
    # CFI). The +onigmo.h+ sentinel in +pre_extract_and_patch!+ keeps
    # this a one-shot edit per extraction; a missing target raises so a future
    # Onigmo pin bump cannot silently skip the fix.
    ST_FOREACH_CALLBACKS = [
      ["i_print_name_entry(UChar* key, NameEntry* e, void* arg)",
       "i_print_name_entry(UChar* key, NameEntry* e, void* arg, int error ARG_UNUSED)"],
      ["i_free_name_entry(UChar* key, NameEntry* e, void* arg ARG_UNUSED)",
       "i_free_name_entry(UChar* key, NameEntry* e, void* arg ARG_UNUSED, int error ARG_UNUSED)"],
      ["i_names(UChar* key ARG_UNUSED, NameEntry* e, INamesArg* arg)",
       "i_names(UChar* key ARG_UNUSED, NameEntry* e, INamesArg* arg, int error ARG_UNUSED)"],
      ["i_renumber_name(UChar* key ARG_UNUSED, NameEntry* e, GroupNumRemap* map)",
       "i_renumber_name(UChar* key ARG_UNUSED, NameEntry* e, GroupNumRemap* map, int error ARG_UNUSED)"]
    ].freeze

    def self.pre_extract_and_patch!
      build_dir = File.join(PROJECT_ROOT, RELATIVE_BUILD_DIR)
      oniguruma_dir = File.join(build_dir, VERSION_DIR)
      return if File.exist?(File.join(oniguruma_dir, "onigmo.h"))

      extract_tarball(build_dir)
      overwrite_config_aux(oniguruma_dir)
      patch_regparse_st_foreach(oniguruma_dir)
    end

    def self.extract_tarball(build_dir)
      tarball = File.join(PROJECT_ROOT, RELATIVE_TARBALL)
      unless File.exist?(tarball)
        raise "[kobako] missing #{tarball} — the mruby-onig-regexp checkout " \
              "(conf.gem github:) did not provide the bundled Onigmo source"
      end

      FileUtils.mkdir_p(build_dir)
      system("tar", "-xzf", tarball, "-C", build_dir, exception: true)
    end

    def self.overwrite_config_aux(oniguruma_dir)
      CONFIG_AUX_SHA256.each_key do |name|
        src = fetch_aux_script(name)
        dst = File.join(oniguruma_dir, name)
        FileUtils.cp(src, dst)
        File.chmod(0o755, dst)
      end
    end

    def self.patch_regparse_st_foreach(oniguruma_dir)
      path = File.join(oniguruma_dir, "regparse.c")
      source = File.read(path)
      ST_FOREACH_CALLBACKS.each do |three_arg, four_arg|
        unless source.include?(three_arg)
          raise "[kobako] Onigmo regparse.c patch target missing: #{three_arg.inspect} " \
                "— the pinned Onigmo source changed; re-verify the st_foreach callback arity fix"
        end
        source = source.sub(three_arg, four_arg)
      end
      File.write(path, source)
    end

    # Returns the cached aux script path, downloading and verifying it
    # on first use. Failing here means Onigmo's pre-wasm +./configure+
    # never runs, which would otherwise blow up downstream with the
    # cryptic +"Invalid configuration 'wasm32-wasi'"+ error.
    def self.fetch_aux_script(name)
      cache = File.join(VENDOR_DIR, ".cache", "#{name}.#{CONFIG_AUX_COMMIT}")
      download_aux_script(name, cache) unless File.exist?(cache)
      verify_aux_checksum(name, cache)
      cache
    end

    # Transient network errors retried by +download_aux_script+. The
    # aux scripts are fetched on every cache-miss CI run, so ride out
    # savannah 5xx hiccups and timeouts; 4xx (URL typo, removed blob)
    # bypasses the retry path and surfaces immediately.
    TRANSIENT_ERRORS = [
      OpenURI::HTTPError, Net::ReadTimeout, Net::OpenTimeout,
      Errno::ECONNRESET, SocketError
    ].freeze

    # Retry attempts wait +2 ** attempt+ seconds (2 + 4 + 8 = 14s total).
    MAX_RETRIES = 3

    def self.download_aux_script(name, dest)
      url = "https://git.savannah.gnu.org/cgit/config.git/plain/#{name}?id=#{CONFIG_AUX_COMMIT}"
      puts "[kobako] downloading #{name}@#{CONFIG_AUX_COMMIT[0, 8]} from #{url}"
      FileUtils.mkdir_p(File.dirname(dest))
      tmp = "#{dest}.part"
      with_retry { URI.parse(url).open("rb") { |io| File.open(tmp, "wb") { |f| IO.copy_stream(io, f) } } }
      File.rename(tmp, dest)
    end

    def self.with_retry
      attempts = 0
      begin
        yield
      rescue *TRANSIENT_ERRORS => e
        raise if permanent?(e) || (attempts += 1) > MAX_RETRIES

        warn "[kobako] retry #{attempts}/#{MAX_RETRIES} after #{e.class}: #{e.message.lines.first&.strip}"
        sleep(2**attempts)
        retry
      end
    end

    def self.permanent?(error)
      error.is_a?(OpenURI::HTTPError) && !error.message.match?(/\A5\d\d\b/)
    end

    def self.verify_aux_checksum(name, path)
      actual = Digest::SHA256.file(path).hexdigest
      expected = CONFIG_AUX_SHA256.fetch(name)
      return if actual == expected

      File.delete(path)
      raise "[kobako] SHA256 mismatch for #{name}@#{CONFIG_AUX_COMMIT[0, 8]}: " \
            "expected #{expected}, got #{actual}"
    end
  end
end
