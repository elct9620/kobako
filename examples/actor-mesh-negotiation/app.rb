# frozen_string_literal: true

# Actor-mesh negotiation demo: a buyer and a seller, each an untrusted
# third-party script, haggle over a price through a host that is the only
# path between them.
#
# Every actor is one Kobako::Sandbox running an untrusted behavior. The
# host owns the mesh runtime — identities, a private per-actor Memory, the
# broker that routes every message, and the transcript. An actor is a pure
# function of the incoming message and the capabilities the host bound to
# its own Sandbox; it can reach nothing else, and it cannot see the
# counterparty's reservation price, memory, or code.
#
# The guest VM is discarded after every turn (a fresh mrb_state per #run),
# so an actor's identity, private state, and capabilities all live host-side
# and outlive the Sandbox instance — the shape that later lets each actor
# run in its own Ractor unchanged.
#
# Usage:
#   ruby examples/actor-mesh-negotiation/app.rb
#   ruby examples/actor-mesh-negotiation/app.rb --buyer-max 1200 --seller-floor 900
#   ruby examples/actor-mesh-negotiation/app.rb --buyer-max 700 --seller-floor 800   # no ZOPA
#
# CLI flags are parsed before bundler/inline resolves dependencies.

require "optparse"

options = { buyer_max: 1000, seller_floor: 800, rounds: 20 }
OptionParser.new do |opts|
  opts.banner = "Usage: ruby examples/actor-mesh-negotiation/app.rb [options]"
  opts.on("--buyer-max N", Integer, "Buyer's private ceiling (default: 1000)") do |value|
    options[:buyer_max] = value
  end
  opts.on("--seller-floor N", Integer, "Seller's private floor (default: 800)") do |value|
    options[:seller_floor] = value
  end
  opts.on("--rounds N", Integer, "Message budget before the talk breaks off (default: 20)") do |value|
    options[:rounds] = value
  end
  opts.on("-h", "--help", "Show this help") do
    warn opts
    exit
  end
end.parse!

require "bundler/inline"

gemfile do
  source "https://rubygems.org"
  gem "kobako", "~> 0.19.0"
end

require "kobako"

# All host-side pieces of the mesh runtime live under one module so the demo
# reads top-down as a single file.
module ActorMesh
  # Wallet exposes an actor's private reservation price. The host injects it
  # and binds it only to that actor's own Sandbox, so the counterparty can
  # never read it — the reservation cannot leak across the mesh.
  class Wallet
    def initialize(reservation)
      @reservation = reservation
    end

    attr_reader :reservation
  end

  # Memory is an actor's private scratch store. Guest VM state is discarded
  # after every invocation, so an actor that must remember anything between
  # turns keeps it here — host-owned and scoped to one actor.
  class Memory
    def initialize
      @store = {}
    end

    def get(key)
      @store[key]
    end

    def set(key, value)
      @store[key] = value
    end
  end

  # An actor in the mesh: a host-owned identity plus the capabilities bound
  # to one Sandbox that runs an untrusted third-party behavior. The behavior
  # is a pure function of the incoming message and this actor's own Wallet /
  # Memory; it reaches nothing the host did not bind.
  class Actor
    attr_reader :name

    def initialize(name:, behavior_path:, reservation:)
      @name = name
      @sandbox = Kobako::Sandbox.new
      @sandbox.bind("Wallet", Wallet.new(reservation))
      @sandbox.bind("Memory", Memory.new)
      @sandbox.preload(code: File.read(behavior_path), name: :Behavior)
    end

    def respond(message)
      @sandbox.run(:Behavior, message)
    end
  end

  # An ordered record of every message the broker delivered, in the exact
  # order it happened — the audit trail a later replay re-runs.
  class Transcript
    def initialize
      @entries = []
    end

    def record(round:, from:, message:)
      @entries << { round: round, from: from, message: message }
    end

    def each(&block)
      @entries.each(&block)
    end
  end

  # The broker is the only path between actors: it delivers each message to
  # its addressee, records the reply, and routes it onward until an actor
  # accepts, rejects, or the round budget runs out. Actors never touch each
  # other directly — the host mediates every exchange.
  class Broker
    def initialize(actors:, max_rounds:, transcript:)
      @actors = actors
      @max_rounds = max_rounds
      @transcript = transcript
    end

    def run
      round = 0
      message = { to: :seller, type: :open }
      while round <= @max_rounds
        reply = deliver(message, round)
        outcome = terminal(reply, round)
        return outcome if outcome

        message = reply
        round += 1
      end
      { status: :no_deal, reason: :exhausted, round: @max_rounds }
    end

    private

    def deliver(message, round)
      recipient = @actors.fetch(message[:to])
      reply = recipient.respond(message.merge(round: round))
      @transcript.record(round: round, from: recipient.name, message: reply)
      reply
    end

    def terminal(reply, round)
      case reply[:type]
      when :accept then { status: :deal, price: reply[:price], round: round }
      when :reject then { status: :no_deal, reason: :rejected, round: round }
      end
    end
  end

  # Renders the negotiation transcript and its outcome for the terminal.
  class Report
    def self.render(transcript, outcome)
      transcript.each { |entry| puts line(entry) }
      puts
      puts summary(outcome)
    end

    def self.line(entry)
      message = entry[:message]
      price = message[:price] ? "$#{message[:price]}" : "-"
      format("  r%<round>02d  %<from>-6s -> %<to>-6s  %<type>-8s %<price>s",
             round: entry[:round], from: entry[:from], to: message[:to],
             type: message[:type], price: price)
    end

    def self.summary(outcome)
      case outcome[:status]
      when :deal then "DEAL at $#{outcome[:price]} (round #{outcome[:round]})"
      else "NO DEAL (#{outcome[:reason]})"
      end
    end
  end

  # Wires the actors from CLI options, runs one negotiation, prints it.
  class Simulation
    ACTORS_DIR = File.join(__dir__, "actors")

    def initialize(options)
      @options = options
    end

    def run
      transcript = Transcript.new
      outcome = Broker.new(actors: build_actors, max_rounds: @options[:rounds],
                           transcript: transcript).run
      Report.render(transcript, outcome)
      outcome
    end

    private

    def build_actors
      {
        seller: Actor.new(name: :seller, behavior_path: behavior("anchor_seller"),
                          reservation: @options[:seller_floor]),
        buyer: Actor.new(name: :buyer, behavior_path: behavior("lowball_buyer"),
                         reservation: @options[:buyer_max])
      }
    end

    def behavior(name)
      File.join(ACTORS_DIR, "#{name}.rb")
    end
  end
end

ActorMesh::Simulation.new(options).run
