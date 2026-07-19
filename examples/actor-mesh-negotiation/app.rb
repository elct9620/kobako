# frozen_string_literal: true

# Actor-mesh negotiation demo: a buyer, a seller, and a settlement actor —
# each an untrusted third-party script in its own Kobako::Sandbox — reach a
# deal through a host that is the only path between them.
#
# The host owns the mesh runtime: per-actor identity, a private Wallet
# (host-injected reservation), Memory, and a seeded Dice; a broker that
# routes every message under an authorization matrix; and a supervisor that
# restarts a faulting actor up to a budget and, once it is spent, makes the
# actor forfeit rather than crash the mesh. Buyer and seller may message
# only each other; neither may address settlement — the host alone invokes
# it, once both sides agree. An actor is a pure function of the incoming
# message and the capabilities bound to its own Sandbox, so it can read
# neither the counterparty's reservation nor its memory nor its code.
#
# The guest VM is discarded after every turn (a fresh mrb_state per #run)
# and the Sandbox is hermetic (no ambient clock or entropy), so an actor's
# state lives host-side and its only randomness is the seeded Dice. That is
# what makes a run reproducible: --replay re-runs it with the same seed and
# checks the transcript comes out byte-for-byte identical.
#
# Usage:
#   ruby examples/actor-mesh-negotiation/app.rb
#   ruby examples/actor-mesh-negotiation/app.rb --buyer-max 700 --seller-floor 800  # no ZOPA
#   ruby examples/actor-mesh-negotiation/app.rb --with-cheater                       # broker blocks it
#   ruby examples/actor-mesh-negotiation/app.rb --with-flaky                         # restarts, recovers
#   ruby examples/actor-mesh-negotiation/app.rb --with-faulty                        # exhausts, forfeits
#   ruby examples/actor-mesh-negotiation/app.rb --with-jitter --replay               # reproduces exactly
#
# CLI flags are parsed before bundler/inline resolves dependencies.

require "optparse"

options = {
  buyer_max: 1000, seller_floor: 800, rounds: 20, seed: 1, restarts: 2,
  with_cheater: false, with_faulty: false, with_flaky: false, with_jitter: false, replay: false
}
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
  opts.on("--seed N", Integer, "Seed for the Dice RNG; only --with-jitter draws from it (default: 1)") do |value|
    options[:seed] = value
  end
  opts.on("--restarts N", Integer, "Restart budget per turn (default: 2)") { |value| options[:restarts] = value }
  opts.on("--with-cheater", "Buyer forges a settlement; the broker blocks it") { options[:with_cheater] = true }
  opts.on("--with-flaky", "Buyer faults once then recovers on restart") { options[:with_flaky] = true }
  opts.on("--with-faulty", "Buyer keeps crashing until it forfeits") { options[:with_faulty] = true }
  opts.on("--with-jitter", "Buyer's bids jitter via the seeded Dice") { options[:with_jitter] = true }
  opts.on("--replay", "Run twice; check the transcript reproduces") { options[:replay] = true }
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

  # Dice is an actor's seeded source of randomness. The Sandbox is hermetic —
  # the guest has no ambient entropy — so an actor that wants to randomize
  # its play must draw from Dice, whose seed the host owns. A fixed seed
  # makes every run reproduce the same rolls, which is what lets a recorded
  # negotiation replay byte-for-byte.
  class Dice
    def initialize(seed)
      @random = Random.new(seed)
    end

    def roll(bound)
      @random.rand(bound)
    end
  end

  # An actor in the mesh: a host-owned identity plus the capabilities bound
  # to one Sandbox that runs an untrusted third-party behavior. The behavior
  # is a pure function of the incoming message and this actor's own
  # capabilities; it reaches nothing the host did not bind. A neutral actor
  # (settlement) is built without a Wallet or Dice, so it holds no
  # reservation and no randomness.
  class Actor
    attr_reader :name

    def initialize(name:, behavior_path:, reservation: nil, seed: nil)
      @name = name
      @sandbox = Kobako::Sandbox.new
      @sandbox.bind("Wallet", Wallet.new(reservation)) unless reservation.nil?
      @sandbox.bind("Memory", Memory.new)
      @sandbox.bind("Dice", Dice.new(seed)) unless seed.nil?
      @sandbox.preload(code: File.read(behavior_path), name: :Behavior)
    end

    def respond(message)
      @sandbox.run(:Behavior, message)
    end
  end

  # The supervisor runs every actor turn under the let-it-crash contract: a
  # fault (a raised SandboxError, or a tripped cap like the wall-clock
  # TrapError) does not crash the mesh. The supervisor restarts the turn up
  # to its budget — a transient fault clears on a later attempt and the
  # negotiation carries on — and an actor that keeps faulting forfeits. Each
  # fault is yielded so the caller can record it.
  class Supervisor
    FAULTS = [Kobako::TrapError, Kobako::SandboxError].freeze

    def initialize(max_restarts:)
      @max_restarts = max_restarts
    end

    def guard(actor, message)
      (0..@max_restarts).each do |restart|
        return [:ok, actor.respond(message)]
      rescue *FAULTS => e
        yield fault(e, restart)
      end
      %i[fault exhausted]
    end

    private

    def fault(error, restart)
      {
        type: :fault, kind: error.class.name.split("::").last,
        detail: error.message.lines.first&.strip,
        attempt: restart + 1, giving_up: restart == @max_restarts
      }
    end
  end

  # An ordered record of every message the broker delivered, in the exact
  # order it happened — the audit trail --replay re-runs and compares.
  class Transcript
    attr_reader :entries

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

  # The broker is the only path between actors. It delivers each message —
  # under the supervisor — to its addressee and routes the reply onward, but
  # only along edges the authorization matrix permits: buyer and seller may
  # message each other, and no one may address settlement (the host alone
  # invokes it, once both sides agree). A reply aimed anywhere else is
  # blocked and recorded, so a forged move never reaches a peer it was
  # denied; an actor that exhausts its restarts forfeits.
  class Broker
    AUTHORIZATION = { buyer: %i[seller], seller: %i[buyer] }.freeze

    def initialize(actors:, settlement:, supervisor:, max_rounds:, transcript:)
      @actors = actors
      @settlement = settlement
      @supervisor = supervisor
      @max_rounds = max_rounds
      @transcript = transcript
    end

    def run
      message = { to: :seller, type: :open }
      (0..@max_rounds).each do |round|
        step, value = turn(message, round)
        return value if step == :done

        message = value
      end
      { status: :no_deal, reason: :exhausted, round: @max_rounds }
    end

    private

    def turn(message, round)
      sender = message[:to]
      status, reply = deliver(sender, message, round)
      return [:done, forfeit(sender, round)] if status == :fault
      return [:done, violation(sender, reply, round)] unless authorized?(sender, reply[:to])

      outcome = terminal(reply, round)
      outcome ? [:done, finalize(outcome)] : [:continue, reply]
    end

    def deliver(to, message, round)
      status, reply = @supervisor.guard(@actors.fetch(to), message.merge(round: round)) do |fault|
        @transcript.record(round: round, from: to, message: fault)
      end
      @transcript.record(round: round, from: to, message: reply) if status == :ok
      [status, reply]
    end

    def authorized?(sender, target)
      AUTHORIZATION.fetch(sender, []).include?(target)
    end

    def forfeit(actor, round)
      { status: :no_deal, reason: :fault, actor: actor, round: round }
    end

    def violation(sender, reply, round)
      @transcript.record(round: round, from: :broker,
                         message: { type: :denied, actor: sender, target: reply[:to] })
      { status: :no_deal, reason: :denied, round: round }
    end

    def terminal(reply, round)
      case reply[:type]
      when :accept then { status: :deal, price: reply[:price], round: round }
      when :reject then { status: :no_deal, reason: :rejected, round: round }
      end
    end

    def finalize(outcome)
      return outcome unless outcome[:status] == :deal

      status, receipt = @supervisor.guard(@settlement, { type: :settle, price: outcome[:price] }) do |fault|
        @transcript.record(round: outcome[:round], from: :settlement, message: fault)
      end
      return { status: :no_deal, reason: :settlement_failed, round: outcome[:round] } if status == :fault

      @transcript.record(round: outcome[:round], from: :settlement, message: receipt)
      outcome.merge(receipt: receipt)
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
      format("  r%<round>02d  %<from>-9s %<detail>s",
             round: entry[:round], from: entry[:from], detail: detail(entry[:message]))
    end

    def self.detail(message)
      case message[:type]
      when :denied then "DENIED  #{message[:actor]} -> #{message[:target]} (broker blocked the actor)"
      when :receipt then "receipt #{message[:id]}  price $#{message[:price]}  fee $#{message[:fee]}"
      when :fault then fault_detail(message)
      else offer_detail(message)
      end
    end

    def self.fault_detail(message)
      action = message[:giving_up] ? "gives up, forfeits" : "supervisor restarts"
      "FAULT   #{message[:kind]}: #{message[:detail]} (attempt #{message[:attempt]}, #{action})"
    end

    def self.offer_detail(message)
      price = message[:price] ? " $#{message[:price]}" : ""
      "#{message[:type]} -> #{message[:to]}#{price}"
    end

    def self.summary(outcome)
      case outcome[:status]
      when :deal then deal_summary(outcome)
      else "NO DEAL (#{outcome[:reason]})"
      end
    end

    def self.deal_summary(outcome)
      receipt = outcome[:receipt]
      "DEAL at $#{outcome[:price]} (round #{outcome[:round]}) — " \
        "settled #{receipt[:id]}, fee $#{receipt[:fee]}, net $#{receipt[:net]}"
    end
  end

  # Wires the actors from CLI options, runs one negotiation, prints it, and —
  # under --replay — runs it a second time with the same seed to confirm the
  # transcript reproduces exactly.
  class Simulation
    ACTORS_DIR = File.join(__dir__, "actors")

    def initialize(options)
      @options = options
    end

    def run
      transcript, outcome = play
      Report.render(transcript, outcome)
      verify_replay(transcript, outcome) if @options[:replay]
      outcome
    end

    private

    def play
      transcript = Transcript.new
      broker = Broker.new(actors: negotiators, settlement: settlement,
                          supervisor: Supervisor.new(max_restarts: @options[:restarts]),
                          max_rounds: @options[:rounds], transcript: transcript)
      [transcript, broker.run]
    end

    def verify_replay(first_transcript, first_outcome)
      transcript, outcome = play
      match = transcript.entries == first_transcript.entries && outcome == first_outcome
      puts
      puts match ? "REPLAY OK — the same seed reproduced the negotiation byte-for-byte" : "REPLAY MISMATCH"
    end

    def negotiators
      {
        seller: Actor.new(name: :seller, behavior_path: behavior("anchor_seller"),
                          reservation: @options[:seller_floor], seed: @options[:seed]),
        buyer: Actor.new(name: :buyer, behavior_path: behavior(buyer_script),
                         reservation: @options[:buyer_max], seed: @options[:seed] + 1)
      }
    end

    def settlement
      Actor.new(name: :settlement, behavior_path: behavior("settlement"))
    end

    def buyer_script
      return "cheater_buyer" if @options[:with_cheater]
      return "faulty" if @options[:with_faulty]
      return "flaky_buyer" if @options[:with_flaky]
      return "jitter_buyer" if @options[:with_jitter]

      "lowball_buyer"
    end

    def behavior(name)
      File.join(ACTORS_DIR, "#{name}.rb")
    end
  end
end

ActorMesh::Simulation.new(options).run
