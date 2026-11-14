# frozen_string_literal: true

require "test_helper"

# E2E — what a script may name a path by. Each literal is what CRuby 3.4 on
# Linux answers.
class TestE2EFilePathConversion < Minitest::Test
  include E2eGuestHelper

  NAMED = <<~RUBY
    by_path = Object.new
    def by_path.to_path; "/m/f"; end
    by_str = Object.new
    def by_str.to_str; "/m/f"; end
    by_both = Object.new
    def by_both.to_path; "/m/f"; end
    def by_both.to_str; "/m/none"; end
    [by_path, by_str, by_both].map { |path| File.read(path) } + [Dir.children(Dir.new("/m/d"))]
  RUBY

  # @behavior FL-101
  def test_a_path_call_takes_what_answers_to_path_or_to_str
    filesystem = Kobako::Filesystem::Tmpfs.new("f" => "hello", "d/x" => "1")
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM).mount("/m", filesystem)

    assert_equal ["hello", "hello", "hello", %w[x]], sandbox.eval(NAMED).value,
                 "File.read and Dir.children through the bundled guest must take an object answering " \
                 "to_path or to_str, to_path before to_str, as CRuby does"
  end
end
