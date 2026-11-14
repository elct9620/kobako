# frozen_string_literal: true

require "test_helper"

# E2E — the paths a script's +Dir.glob+ and +Dir[]+ patterns name beneath a
# Mount. Each literal is what CRuby 3.4 on Linux answers for the same tree.
class TestE2EDirGlob < Minitest::Test
  include E2eGuestHelper

  # The class each call raises, or what it answers.
  ATTEMPT = "attempt = ->(call) { begin; call.call; rescue Exception => e; e.class.to_s; end }\n"

  MATCHED = <<~RUBY
    Dir.chdir("/t")
    ["*", "**/*.rb", "{a,{b,Z}}.*", "?.rb", "[!a].*", "[a-c].*", "*/", "**/", "sub/**/", ".*",
     "sub/.*", "/t/*.rb", "./*.rb", "sub/../*.rb", "A.RB", "{a.rb", "[a.rb", "\\\\a.rb", "nope/*"]
      .map { |pattern| Dir.glob(pattern) } +
      [Dir.glob("*", File::FNM_DOTMATCH), Dir.glob("**/*.rb", File::FNM_DOTMATCH)]
  RUBY

  DOTTED = <<~RUBY
    Dir.chdir("/t")
    ["**/.*", "**/.", "sub/**/.*"].map { |pattern| Dir.glob(pattern) } +
      ["**/*", "*/*", "sub/*", "**/"].map { |pattern| Dir.glob(pattern, File::FNM_DOTMATCH) }
  RUBY

  CHOSEN = <<~RUBY
    Dir.chdir("/t")
    yielded = []
    [Dir.glob("*.rb", base: "sub"), Dir.glob("*", File::FNM_DOTMATCH, base: "/t/sub"),
     Dir.glob("/t/*.rb", base: "/x"), Dir.glob("*", sort: false).sort,
     Dir.glob("\\\\a.rb", File::FNM_NOESCAPE), Dir.glob(["*.txt", "*.rb"]), Dir["*.rb", "*.txt"],
     Dir["*.txt", base: "/t"], Dir.glob("*.rb") { |path| yielded << path }, yielded]
  RUBY

  REFUSED = <<~RUBY
    [-> { Dir.glob("a\\0b") }, -> { Dir.glob(1) }, -> { Dir.glob("*", sort: nil) }].map(&attempt)
  RUBY

  def sandbox
    files = %w[a.rb b.txt .hidden sub/c.rb sub/.d.rb sub/deep/e.rb Z.rb].to_h { |path| [path, "x"] }
    Kobako::Sandbox.new(wasm_path: REAL_WASM).mount("/t", Kobako::Filesystem::Tmpfs.new(files))
  end

  # @behavior FL-092
  def test_a_script_finds_the_paths_a_pattern_names
    rb = %w[Z.rb a.rb]
    assert_equal [%w[Z.rb a.rb b.txt sub], %w[Z.rb a.rb sub/c.rb sub/deep/e.rb], %w[a.rb b.txt Z.rb], rb,
                  %w[Z.rb b.txt], %w[a.rb b.txt], %w[sub/], %w[sub/ sub/deep/], %w[sub/ sub/deep/],
                  %w[. .hidden], %w[sub/. sub/.d.rb], %w[/t/Z.rb /t/a.rb], %w[./Z.rb ./a.rb],
                  %w[sub/../Z.rb sub/../a.rb], [], [], [], %w[a.rb], [],
                  %w[. .hidden Z.rb a.rb b.txt sub], %w[Z.rb a.rb sub/.d.rb sub/c.rb sub/deep/e.rb]],
                 sandbox.eval(MATCHED).value,
                 "Dir.glob through the bundled guest must answer CRuby's paths, in CRuby's order, for " \
                 "wildcards, brackets, braces, **/, a trailing separator, and FNM_DOTMATCH"
  end

  # @behavior FL-092
  def test_a_pattern_answers_a_directory_as_its_own_dot_only_where_crubys_does
    assert_equal [%w[.hidden sub/.d.rb], [], %w[sub/.d.rb],
                  %w[. .hidden Z.rb a.rb b.txt sub sub/.d.rb sub/c.rb sub/deep sub/deep/e.rb],
                  %w[./.hidden ./Z.rb ./a.rb ./b.txt ./sub sub/.d.rb sub/c.rb sub/deep],
                  %w[sub/. sub/.d.rb sub/c.rb sub/deep], %w[sub/ sub/deep/]],
                 sandbox.eval(DOTTED).value,
                 "Dir.glob through the bundled guest must answer a directory as its own . only in the " \
                 "first directory a pattern lists, and never under **/ without FNM_DOTMATCH, as CRuby does"
  end

  # @behavior FL-093
  def test_a_patterns_options_choose_where_it_reads_and_what_it_answers
    assert_equal [%w[c.rb], %w[. .d.rb c.rb deep], %w[/t/Z.rb /t/a.rb], %w[Z.rb a.rb b.txt sub], [],
                  %w[b.txt Z.rb a.rb], %w[Z.rb a.rb b.txt], %w[b.txt], nil, %w[Z.rb a.rb]],
                 sandbox.eval(CHOSEN).value,
                 "Dir.glob with base:, sort: false, FNM_NOESCAPE, an Array, or a block, and Dir[] through " \
                 "the bundled guest must answer CRuby's paths, a block yielding each and answering nil"
  end

  # @behavior FL-094
  def test_a_pattern_walk_follows_a_link_only_where_a_wildcard_names_it
    skip "this host makes no links" if Gem.win_platform?
    TestPaths.host_directory do |host|
      assert_equal [%w[sub/deep/e.rb], %w[link/ sub/], %w[link/deep/e.rb sub/deep/e.rb]],
                   linked(host).eval('Dir.chdir("/t"); ["**/e.rb", "*/", "*/**/e.rb"].map { |p| Dir.glob(p) }').value,
                   "Dir.glob through the bundled guest must leave a linked directory to **/ and descend " \
                   "through it where a wildcard names it, as CRuby does"
    end
  end

  # A Sandbox binding +host+ at /t, holding +sub/deep/e.rb+ and +link+ to +sub+.
  def linked(host)
    FileUtils.mkdir_p(File.join(host, "sub", "deep"))
    File.write(File.join(host, "sub", "deep", "e.rb"), "x")
    File.symlink("sub", File.join(host, "link"))
    Kobako::Sandbox.new(wasm_path: REAL_WASM).mount("/t", Kobako::Filesystem::Bind.new(host))
  end

  # @behavior FL-095
  def test_a_pattern_a_glob_cannot_take_raises_as_crubys_does
    assert_equal %w[ArgumentError TypeError ArgumentError], sandbox.eval(ATTEMPT + REFUSED).value,
                 "Dir.glob through the bundled guest must refuse a NUL and sort: nil with ArgumentError, " \
                 "and an Integer pattern with TypeError"
  end
end
