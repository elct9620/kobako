# frozen_string_literal: true

require_relative "capture"
require_relative "usage"

module Kobako
  # Kobako::Execution — the frozen result of one +Sandbox#eval+ / +#run+: the
  # decoded +#value+ plus the run's output captures and +#usage+. A successful
  # run returns it; a failed run raises an error carrying the same frozen
  # Execution on the error's +#execution+, so a rescue reads the captures and
  # usage exactly as a successful caller reads them off the return value. On a
  # failed run +#value+ is +nil+ — only the captures and usage are meaningful.
  #
  # +#failed?+ tells the returned Execution from the carried one even when both
  # +#value+ are +nil+ (a run whose last expression was +nil+ versus a failed
  # one). It holds no reference to the raising error, so the error carries the
  # Execution but not the reverse.
  class Execution
    # The deserialized guest value the run produced; +nil+ on a failed run.
    attr_reader :value

    # The +Kobako::Usage+ resource accounting for this run.
    attr_reader :usage

    def initialize(value:, usage:, stdout:, stderr:, failed:)
      @value = value
      @usage = usage
      @stdout_capture = stdout
      @stderr_capture = stderr
      @failed = failed
      freeze
    end

    # Returns +true+ iff the run failed — +false+ on the Execution +#eval+ /
    # +#run+ returned, +true+ on the one a raised error carries.
    def failed? = @failed

    # Bytes the guest wrote to stdout during this run as a UTF-8 String,
    # clipped at +stdout_limit+; the content carries no truncation sentinel,
    # so use +#stdout_truncated?+ to observe overflow.
    def stdout = @stdout_capture.bytes

    # Bytes the guest wrote to stderr during this run. Mirror of #stdout.
    def stderr = @stderr_capture.bytes

    # Returns +true+ iff stdout capture reached +stdout_limit+ during this run.
    def stdout_truncated? = @stdout_capture.truncated?

    # Returns +true+ iff stderr capture reached +stderr_limit+ during this run.
    def stderr_truncated? = @stderr_capture.truncated?
  end
end
