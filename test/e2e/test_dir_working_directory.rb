# frozen_string_literal: true

require "test_helper"

# E2E — the working directory a script reads with +Dir.pwd+ and changes with
# +Dir.chdir+ beneath a Mount. Each literal is what CRuby 3.4 on Linux
# answers, unless a comment says why not.
class TestE2EDirWorkingDirectory < Minitest::Test
  include E2eGuestHelper

  # The class each call raises, or what it answers.
  ATTEMPT = "attempt = ->(call) { begin; call.call; rescue Exception => e; e.class.to_s; end }\n"

  CHANGED = <<~RUBY
    [Dir.pwd, Dir.chdir("/m"), Dir.getwd, Dir.chdir("sub"), Dir.pwd, File.read("c.rb"),
     Dir.children("deep"), Dir.glob("*"), Dir.chdir(".."), Dir.pwd]
  RUBY

  WITHIN_A_BLOCK = <<~RUBY
    answered = Dir.chdir("/m/sub") { |path| [path, Dir.pwd, File.read("c.rb")] }
    raised = begin; Dir.chdir("/m/sub") { raise "x" }; rescue RuntimeError; Dir.pwd; end
    [answered, raised, Dir.pwd]
  RUBY

  REFUSED = <<~RUBY
    Dir.chdir("/m")
    [-> { Dir.chdir("/m/none") }, -> { Dir.chdir("/m/f") }, -> { Dir.chdir }].map(&attempt) + [Dir.pwd]
  RUBY

  def sandbox
    filesystem = Kobako::Filesystem::Tmpfs.new("f" => "x", "sub/c.rb" => "c", "sub/deep/e.rb" => "e")
    Kobako::Sandbox.new(wasm_path: REAL_WASM).mount("/m", filesystem)
  end

  # @behavior FL-096
  def test_a_script_changes_its_working_directory
    assert_equal ["/", 0, "/m", 0, "/m/sub", "c", %w[e.rb], %w[c.rb deep], 0, "/m"], sandbox.eval(CHANGED).value,
                 "Dir.chdir through the bundled guest must answer 0 and resolve each relative path from the " \
                 "working directory Dir.pwd names, starting at / and stepping back with .."
  end

  # @behavior FL-097
  def test_a_block_changes_the_working_directory_only_while_it_runs
    assert_equal [["/m/sub", "/m/sub", "c"], "/", "/"], sandbox.eval(WITHIN_A_BLOCK).value,
                 "Dir.chdir with a block through the bundled guest must run the block there, answer its " \
                 "value, and restore the earlier directory however the block ends"
  end

  # @behavior FL-098
  def test_each_invocation_starts_at_the_root_directory
    mounted = sandbox
    mounted.eval('Dir.chdir("/m/sub")')

    assert_equal "/", mounted.eval("Dir.pwd").value,
                 "a later Sandbox#eval must start at / whatever directory an earlier one changed into"
  end

  # @behavior FL-099
  def test_a_change_into_what_is_no_directory_raises_as_crubys_does
    assert_equal ["Errno::ENOENT", "Errno::ENOTDIR", "ArgumentError", "/m"], sandbox.eval(ATTEMPT + REFUSED).value,
                 "Dir.chdir through the bundled guest must raise ENOENT for a missing path, ENOTDIR for a " \
                 "file, and ArgumentError for no path, leaving the working directory where it was"
  end

  # CRuby on Linux answers "/t/sub", the directory the link reaches: the guest
  # reads no link's target, so it names the path it was given.
  # @behavior FL-100
  def test_a_directory_reached_through_a_link_is_named_by_the_links_path
    skip "this host makes no links" if Gem.win_platform?
    TestPaths.host_directory do |host|
      Dir.mkdir(File.join(host, "sub"))
      File.symlink("sub", File.join(host, "link"))
      sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM).mount("/t", Kobako::Filesystem::Bind.new(host))

      assert_equal "/t/link", sandbox.eval('Dir.chdir("/t/link"); Dir.pwd').value,
                   "Dir.pwd through the bundled guest after Dir.chdir through a link must name the link's path"
    end
  end
end
