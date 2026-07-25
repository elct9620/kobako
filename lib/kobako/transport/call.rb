# frozen_string_literal: true

module Kobako
  module Transport
    # One guest-initiated dispatch, as the native side decoded it off the
    # core envelope. The routing fields arrive already decoded — a String
    # for a bound constant's path, an Integer for a Capability Handle id —
    # so the host resolves a target without interpreting a payload byte.
    #
    # +payload+ stays bytes here on purpose: only the Dispatcher decodes
    # it, and only through +Kobako::Payload::Arguments+, which keeps a
    # large argument's strings shared with the buffer the ext handed over.
    #
    # Built on the +class X < Data.define(...)+ subclass form so the class
    # body is fully Steep-visible; see +lib/kobako/outcome/panic.rb+ for
    # the rationale.
    class Call < Data.define(:target, :method_name, :block_given, :payload)
    end
  end
end
