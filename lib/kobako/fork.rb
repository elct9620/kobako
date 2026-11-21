# frozen_string_literal: true

module Kobako
  # The check a fork through Ruby passes, defined by the ext: +prepare+ waits
  # for every run in progress and raises RuntimeError for a fork that cannot
  # go on, and +release+ lets runs in again. This file hooks it into each
  # fork Ruby makes.
  module Fork # :nodoc:
    # Prepended to Process's singleton class. +_fork+ is what Kernel#fork,
    # Process.fork, and <tt>IO.popen("-")</tt> call, and Process.daemon
    # forks without it. Only the main Ractor runs a Sandbox, so another
    # Ractor's fork has nothing to wait for.
    module Hook
      def _fork
        Fork.around { super }
      end

      def daemon(...)
        Fork.around { super }
      end
    end

    # Run +fork+ checked. A forked child holds no run back, so its +release+
    # finds nothing to let in.
    def self.around
      return yield unless Ractor.current == Ractor.main

      prepare
      begin
        yield
      ensure
        release
      end
    end

    # Hook every fork from now on, once. A Sandbox installs it as it is
    # built, so the hook stands outside the ones loaded before.
    def self.install
      Process.singleton_class.prepend(Hook) unless Process.singleton_class <= Hook
    end
  end
end
