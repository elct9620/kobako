# frozen_string_literal: true

# Shared skip / flunk guards for tests that need the compiled native ext and,
# sometimes, a built Guest Binary or a committed fixture. A clean checkout
# before `rake compile` / `rake wasm:build` is missing those artifacts, so
# the tests skip with a pointer at the build step; under CI the default task
# builds everything first, so a miss there is a broken pipeline, not a skip.
#
# One module so every guard reads the same way: mix it in and call the guard
# that matches what the test drives.
module GuestGuard
  # The native ext must be compiled.
  def require_native_ext!
    return if defined?(Kobako::Runtime)

    flunk "native ext not compiled under CI" if ENV["CI"]
    skip "native ext not compiled (run `bundle exec rake compile`)"
  end

  # The ext plus a built Guest Binary artifact — the pure guest or a variant.
  # +build+ is the rake task that produces it.
  def require_guest_binary!(path, build:)
    require_native_ext!
    return if File.exist?(path)

    flunk "#{File.basename(path)} missing under CI" if ENV["CI"]
    skip "#{File.basename(path)} missing — run `#{build}`"
  end

  # The ext plus a committed test fixture (never built), so a miss is an odd
  # local state a plain skip covers.
  def require_fixture!(path)
    require_native_ext!
    skip "#{File.basename(path)} fixture missing" unless File.exist?(path)
  end
end
