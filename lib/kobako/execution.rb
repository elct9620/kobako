# frozen_string_literal: true

require_relative "capture"
require_relative "usage"

module Kobako
  # The record of one Sandbox#eval or Sandbox#run: the guest's #value, its
  # output, and its #usage. A failed run raises instead, and the error carries
  # the same record on +execution+, where #value is +nil+.
  #
  # #failed? tells the two apart when both #value are +nil+: a script whose
  # last expression was +nil+ versus one that failed.
  class Execution
    # The guest value the run produced; +nil+ on a failed run.
    attr_reader :value

    # What the run spent against its caps, as a Usage.
    attr_reader :usage

    def initialize(value:, usage:, stdout:, stderr:, failed:) # :nodoc:
      @value = value
      @usage = usage
      @stdout_capture = stdout
      @stderr_capture = stderr
      @failed = failed
      freeze
    end

    # Whether the run failed: +false+ on the Execution #eval or #run
    # returned, +true+ on the one a raised error carries.
    def failed? = @failed

    # What the guest wrote to stdout, up to +stdout_limit+ bytes: a UTF-8
    # String, or a binary one when the bytes are not valid UTF-8. The text
    # carries no sign it was cut; #stdout_truncated? says so.
    def stdout = @stdout_capture.bytes

    # What the guest wrote to stderr; see #stdout.
    def stderr = @stderr_capture.bytes

    # Whether stdout reached +stdout_limit+.
    def stdout_truncated? = @stdout_capture.truncated?

    # Whether stderr reached +stderr_limit+.
    def stderr_truncated? = @stderr_capture.truncated?
  end
end
