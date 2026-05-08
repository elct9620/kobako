# frozen_string_literal: true

# E2E test for tasks/vendor.rake — exercises the full download, checksum, and
# extraction flow against a local HTTP fixture serving fake tarballs.
#
# Intentionally does NOT require "test_helper": that file loads the native
# extension, which doesn't exist in clean checkouts. This test only invokes
# `rake` as a subprocess, so it doesn't need any Kobako Ruby code at runtime.

require "minitest/autorun"
require "digest"
require "fileutils"
require "open3"
require "rbconfig"
require "socket"
require "tmpdir"
require "webrick"
require "rubygems/package"
require "stringio"
require "zlib"

class TestVendorTask < Minitest::Test
  PROJECT_ROOT = File.expand_path("..", __dir__)

  # The constants the rake task expects in its tarball filenames. Must stay
  # in sync with tasks/vendor.rake; if those constants drift, this test will
  # fail loudly, which is exactly the point.
  WASI_SDK_VERSION      = "26"
  WASI_SDK_FULL_VERSION = "26.0"
  MRUBY_VERSION         = "3.3.0"

  WASI_SDK_PLATFORM =
    case RUBY_PLATFORM
    when /arm64-darwin|aarch64-darwin/ then "arm64-macos"
    when /x86_64-darwin/               then "x86_64-macos"
    when /aarch64-linux|arm64-linux/   then "arm64-linux"
    when /x86_64-linux/                then "x86_64-linux"
    else "x86_64-linux"
    end

  WASI_SDK_TARBALL_NAME = "wasi-sdk-#{WASI_SDK_FULL_VERSION}-#{WASI_SDK_PLATFORM}.tar.gz".freeze
  WASI_SDK_UNPACKED_DIR = "wasi-sdk-#{WASI_SDK_FULL_VERSION}-#{WASI_SDK_PLATFORM}".freeze

  MRUBY_TARBALL_NAME = "mruby-#{MRUBY_VERSION}.tar.gz".freeze
  MRUBY_UNPACKED_DIR = "mruby-#{MRUBY_VERSION}".freeze

  def setup
    @tmpdir = Dir.mktmpdir("kobako-vendor-test-")
    @serve_dir = File.join(@tmpdir, "serve")
    @vendor_dir = File.join(@tmpdir, "vendor")
    FileUtils.mkdir_p([@serve_dir, @vendor_dir])

    build_wasi_fixture
    build_mruby_fixture

    @server, @port = start_server(@serve_dir)
  end

  def teardown
    @server&.shutdown
    @server_thread&.join
    FileUtils.remove_entry(@tmpdir) if @tmpdir && File.exist?(@tmpdir)
  end

  # End-to-end: real `rake vendor:setup` against a local HTTP fixture.
  # Asserts download, checksum, extraction, and idempotent re-run.
  def test_setup_downloads_verifies_extracts_and_is_idempotent
    out, status = run_rake_setup

    assert status.success?, "first vendor:setup run failed:\n#{out}"

    cache_dir = File.join(@vendor_dir, ".cache")
    wasi_cached = File.join(cache_dir, WASI_SDK_TARBALL_NAME)
    mruby_cached = File.join(cache_dir, MRUBY_TARBALL_NAME)

    assert File.exist?(wasi_cached), "wasi-sdk tarball must be cached at #{wasi_cached}"
    assert File.exist?(mruby_cached), "mruby tarball must be cached at #{mruby_cached}"

    # Sentinel files prove extraction reached the expected target dirs.
    assert File.exist?(File.join(@vendor_dir, "wasi-sdk", "bin", "clang")),
           "wasi-sdk should expose bin/clang sentinel"
    assert File.exist?(File.join(@vendor_dir, "mruby", "Rakefile")),
           "mruby tree should expose Rakefile sentinel"

    # SHA-256 sidecars confirm checksum verification ran.
    assert File.exist?("#{wasi_cached}.sha256"), "wasi-sdk sha sidecar must be written"
    assert File.exist?("#{mruby_cached}.sha256"), "mruby sha sidecar must be written"
    assert_equal @wasi_sha, File.read("#{wasi_cached}.sha256").strip
    assert_equal @mruby_sha, File.read("#{mruby_cached}.sha256").strip

    # Idempotency: snapshot mtimes, re-run, mtimes must not change.
    before = sentinel_mtimes
    out2, status2 = run_rake_setup
    assert status2.success?, "second vendor:setup run failed:\n#{out2}"
    after = sentinel_mtimes

    assert_equal before, after, "re-running vendor:setup must be a no-op"
  end

  # Checksum mismatch must abort: pin the SHA via env to a wrong value and
  # expect the task to fail loudly.
  def test_setup_rejects_checksum_mismatch
    out, status = run_rake_setup(extra_env: { "KOBAKO_VENDOR_WASI_SDK_SHA256" => "0" * 64 })

    refute status.success?, "vendor:setup must fail on bad wasi-sdk checksum"
    assert_match(/checksum mismatch/, out)
  end

  private

  def sentinel_mtimes
    {
      "wasi" => File.mtime(File.join(@vendor_dir, "wasi-sdk", "bin", "clang")),
      "mruby" => File.mtime(File.join(@vendor_dir, "mruby", "Rakefile"))
    }
  end

  def build_wasi_fixture
    path = File.join(@serve_dir, WASI_SDK_TARBALL_NAME)
    write_tar_gz(
      path,
      [
        ["#{WASI_SDK_UNPACKED_DIR}/bin/clang", "#!/bin/sh\nexit 0\n"],
        ["#{WASI_SDK_UNPACKED_DIR}/share/wasi-sysroot/include/.keep", ""]
      ]
    )
    @wasi_sha = Digest::SHA256.file(path).hexdigest
  end

  def build_mruby_fixture
    path = File.join(@serve_dir, MRUBY_TARBALL_NAME)
    write_tar_gz(
      path,
      [
        ["#{MRUBY_UNPACKED_DIR}/Rakefile", "# fake mruby\n"],
        ["#{MRUBY_UNPACKED_DIR}/include/mruby.h", "/* fake */\n"]
      ]
    )
    @mruby_sha = Digest::SHA256.file(path).hexdigest
  end

  # Build a .tar.gz with the given [path, content] entries using the
  # gem-bundled Gem::Package::TarWriter (no shelling out).
  def write_tar_gz(out_path, entries)
    raw = StringIO.new(+"".b)
    Gem::Package::TarWriter.new(raw) do |tar|
      entries.each do |path, content|
        tar.add_file(path, 0o644) { |io| io.write(content) }
      end
    end
    File.open(out_path, "wb") do |f|
      gz = Zlib::GzipWriter.new(f)
      gz.write(raw.string)
      gz.close
    end
  end

  def start_server(docroot)
    port = pick_port
    server = WEBrick::HTTPServer.new(
      Port: port,
      DocumentRoot: docroot,
      Logger: WEBrick::Log.new(File::NULL),
      AccessLog: []
    )
    @server_thread = Thread.new { server.start }
    [server, port]
  end

  def pick_port
    s = TCPServer.new("127.0.0.1", 0)
    port = s.addr[1]
    s.close
    port
  end

  def run_rake_setup(extra_env: {})
    base_url = "http://127.0.0.1:#{@port}"
    env = {
      "KOBAKO_VENDOR_BASE_URL" => base_url,
      "KOBAKO_VENDOR_WASI_SDK_SHA256" => @wasi_sha,
      "KOBAKO_VENDOR_MRUBY_SHA256" => @mruby_sha,
      "KOBAKO_VENDOR_DIR" => @vendor_dir
    }.merge(extra_env)

    # Run rake from a tmp project root that loads only tasks/vendor.rake and
    # points VENDOR_DIR at our scratch dir, so the test never writes into the
    # real repo's vendor/.
    rakefile = File.join(@tmpdir, "Rakefile")
    File.write(rakefile, <<~RUBY)
      load #{File.join(PROJECT_ROOT, "tasks", "vendor.rake").inspect}
    RUBY

    Open3.capture2e(env, RbConfig.ruby, "-S", "rake", "vendor:setup", chdir: @tmpdir)
  end
end
