# frozen_string_literal: true

require "test_helper"

# E2E — the Layers Overlay.new takes, refused in Ruby before any guest runs.
class TestE2EOverlay < Minitest::Test
  # Only a Tmpfs holds an Overlay's markers, as gVisor takes only such an
  # upper, so even a writable host directory is refused there.
  def test_an_overlay_refuses_an_upper_that_is_no_tmpfs
    bind = Kobako::Filesystem::Bind.new(Dir.tmpdir)
    assert_raises(TypeError, "Overlay.new with a Bind as its upper must raise TypeError") do
      Kobako::Filesystem::Overlay.new(lower: [Kobako::Filesystem::Tmpfs.new], upper: bind)
    end
  end

  def test_an_overlay_without_an_upper_needs_two_lower_layers
    assert_raises(ArgumentError, "Overlay.new with one lower Layer and no upper must raise ArgumentError") do
      Kobako::Filesystem::Overlay.new(lower: [Kobako::Filesystem::Tmpfs.new])
    end
  end
end
