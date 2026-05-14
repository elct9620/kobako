# frozen_string_literal: true

require "fileutils"
require "open-uri"
require "net/http" # eager — DOWNLOAD_TRANSIENT_ERRORS names Net::* at module-eval

module KobakoVendor
  # Tarball download with exponential-backoff retry on transient
  # network failures. Owns +DOWNLOAD_TRANSIENT_ERRORS+ and the retry
  # cap; the .rake wrapper just calls +KobakoVendor::Download.download+.
  module Download
    # Retry attempts wait +2 ** attempt+ seconds (2 + 4 + 8 = 14s total)
    # — enough to ride out a GitHub archive 502 / TCP read timeout.
    MAX_RETRIES = 3

    # Transient network errors retried by +with_retry+. +OpenURI::HTTPError+
    # is narrowed to 5xx; 4xx (URL typo, deleted repo) bypasses the retry
    # path.
    TRANSIENT_ERRORS = [
      OpenURI::HTTPError, Net::ReadTimeout, Net::OpenTimeout,
      Errno::ECONNRESET, SocketError
    ].freeze

    def self.download(url, dest)
      FileUtils.mkdir_p(File.dirname(dest))
      tmp = "#{dest}.part"
      with_retry do
        URI.parse(url).open("rb") { |io| File.open(tmp, "wb") { |f| IO.copy_stream(io, f) } }
      end
      File.rename(tmp, dest)
    end

    # Exponential-backoff retry wrapper for transient download failures.
    # +OpenURI::HTTPError+ is narrowed to 5xx so 4xx (URL typo, repo
    # deleted, expired ref) bypasses the retry path and surfaces
    # immediately.
    def self.with_retry
      attempts = 0
      begin
        yield
      rescue *TRANSIENT_ERRORS => e
        raise if permanent?(e) || (attempts += 1) > MAX_RETRIES

        warn_and_sleep(e, attempts)
        retry
      end
    end

    def self.permanent?(error)
      error.is_a?(OpenURI::HTTPError) && !error.message.match?(/\A5\d\d\b/)
    end

    def self.warn_and_sleep(error, attempt)
      warn "[vendor] retry #{attempt}/#{MAX_RETRIES} after #{error.class}: " \
           "#{error.message.lines.first&.strip}"
      sleep(2**attempt)
    end
  end
end
