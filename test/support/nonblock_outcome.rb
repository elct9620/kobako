# frozen_string_literal: true

# How the journeys that read and write a socket without waiting describe what
# a call raises.
module NonblockOutcome
  # A script's opening that describes what a call raises as its class, its
  # message, and whether it is an Errno::EAGAIN, an IO::WaitReadable, and an
  # IO::WaitWritable.
  SCRIPT = <<~RUBY
    outcome = lambda do |&call|
      call.call
    rescue StandardError => e
      [e.class.to_s, e.message, e.is_a?(Errno::EAGAIN), e.is_a?(IO::WaitReadable), e.is_a?(IO::WaitWritable)]
    end
  RUBY

  # What SCRIPT describes a +call+ that would wait going +direction+ as, the
  # way CRuby words it.
  def self.would_block(call, direction: :read)
    readable = direction == :read
    ["IO::EAGAINWait#{readable ? "Readable" : "Writable"}", "Resource temporarily unavailable - #{call}", true,
     readable, !readable]
  end

  # What SCRIPT describes a raise that is no wait as.
  def self.plain_raise(class_name, message)
    [class_name, message, false, false, false]
  end
end
