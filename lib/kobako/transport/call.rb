# frozen_string_literal: true

module Kobako
  module Transport
    # One guest-initiated dispatch, as the native side decoded it off the
    # core envelope. The routing fields arrive already decoded — a String
    # for a bound constant's path, an Integer for a Capability Handle id —
    # so the host resolves a target without interpreting a payload byte.
    #
    # +payload+ stays bytes here on purpose: routing never interprets a
    # payload byte, and the one place that does — the Dispatcher — must
    # decode it inside the Codec brackets that keep a Fault out of a
    # payload position and record whether a Handle crossed.
    #
    # Built on the +class X < Data.define(...)+ subclass form so the class
    # body is fully Steep-visible; see +.rubocop.yml+ for the
    # rationale.
    class Call < Data.define(:target, :method_name, :block_given, :payload)
    end
  end
end
