# frozen_string_literal: true

# Actor-mesh negotiation demo: one seller and N competing buyers — each an
# untrusted third-party script in its own Kobako::Sandbox — plus a
# settlement actor, all reaching a deal through a host that is the only path
# between them.
#
# The host owns the mesh runtime: per-actor identity, a private Wallet
# (host-injected reservation), Memory, and a seeded Dice; a broker that runs
# a sealed auction — it broadcasts the seller's ask, collects each buyer's
# bid, and lets the seller award the sale — under an authorization matrix;
# and a supervisor that restarts a faulting actor and, once its budget is
# spent, drops it so the auction carries on. Buyers may bid only to the
# seller; they never see each other's bids and cannot address settlement —
# the host alone invokes it, once the seller awards. An actor is a pure
# function of the incoming message and the capabilities bound to its own
# Sandbox, so it can read neither a rival's reservation nor its memory nor
# its code.
#
# The guest VM is discarded after every turn (a fresh mrb_state per #run)
# and the Sandbox is hermetic (no ambient clock or entropy), so an actor's
# state lives host-side and its only randomness is the seeded Dice. That is
# what makes a run reproducible: --replay re-runs it with the same seed and
# checks the transcript comes out byte-for-byte identical.
#
# Two well-behaved buyers (lowball and jitter) always compete; each
# --with-<type> adds one more, misbehaving buyer on top, so the mesh can be
# watched handling it while the auction still reaches a deal.
#
# Usage:
#   ruby examples/actor-mesh-negotiation/app.rb                          # two buyers compete
#   ruby examples/actor-mesh-negotiation/app.rb --with-flaky             # +1 that recovers on restart
#   ruby examples/actor-mesh-negotiation/app.rb --with-faulty            # +1 that drops out
#   ruby examples/actor-mesh-negotiation/app.rb --with-cheater           # +1 the broker blocks
#   ruby examples/actor-mesh-negotiation/app.rb --replay                 # reproduces exactly
#
# CLI flags are parsed before bundler/inline resolves dependencies.

require "optparse"

options = {
  buyer_max: 1000, seller_floor: 800, rounds: 20, seed: 1, restarts: 2, buyers: [], replay: false
}
OptionParser.new do |opts|
  opts.banner = "Usage: ruby examples/actor-mesh-negotiation/app.rb [options]"
  opts.on("--buyer-max N", Integer, "Every buyer's private ceiling (default: 1000)") do |value|
    options[:buyer_max] = value
  end
  opts.on("--seller-floor N", Integer, "Seller's private floor (default: 800)") do |value|
    options[:seller_floor] = value
  end
  opts.on("--rounds N", Integer, "Auction rounds before it breaks off (default: 20)") do |value|
    options[:rounds] = value
  end
  opts.on("--seed N", Integer, "Seed for the Dice RNG a jitter buyer draws from (default: 1)") do |value|
    options[:seed] = value
  end
  opts.on("--restarts N", Integer, "Restart budget per turn (default: 2)") { |value| options[:restarts] = value }
  opts.on("--with-flaky", "Add a buyer that faults once then recovers") { options[:buyers] << "flaky_buyer" }
  opts.on("--with-faulty", "Add a buyer that keeps crashing and drops out") { options[:buyers] << "faulty" }
  opts.on("--with-cheater", "Add a buyer that bids at settlement (broker blocks it)") do
    options[:buyers] << "cheater_buyer"
  end
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
  # and binds it only to that actor's own Sandbox, so a rival can never read
  # it — the reservation cannot leak across the mesh.
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
  # auction replay byte-for-byte.
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
  # to its budget — a transient fault clears on a later attempt — and an
  # actor that keeps faulting is reported spent, so the broker can drop it.
  # Each fault is yielded so the caller can record it.
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
      %i[fault spent]
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

  # The broker runs the auction and is the only path between actors. Each
  # round it broadcasts the seller's ask, collects one bid per active buyer,
  # and lets the seller either award the sale or lower its ask. It enforces
  # the authorization matrix — a buyer may bid only to the seller, so a bid
  # aimed at settlement (or a peer) is blocked and that buyer drops — and it
  # drops any buyer the supervisor reports as spent, so one crash does not
  # end the auction. The host alone invokes settlement, once the seller
  # awards.
  class Broker
    def initialize(cast:, supervisor:, max_rounds:, transcript:)
      @seller = cast.fetch(:seller)
      @settlement = cast.fetch(:settlement)
      @supervisor = supervisor
      @max_rounds = max_rounds
      @transcript = transcript
      @active = cast.fetch(:buyers).dup
    end

    def run
      ask = open_ask
      return no_deal(:seller_failed, 0) unless ask

      (1..@max_rounds).each do |round|
        step, value = auction_round(ask, round)
        return value if step == :done

        ask = value
      end
      no_deal(:exhausted, @max_rounds)
    end

    private

    def auction_round(ask, round)
      bids = collect_bids(ask, round)
      return [:done, no_deal(:all_dropped, round)] if @active.empty?

      decision = ask_seller(bids, round)
      return [:done, no_deal(:seller_failed, round)] unless decision
      return [:done, settle(decision, round)] if decision[:type] == :accept

      [:continue, decision[:price]]
    end

    def open_ask
      status, reply = invoke(@seller, { type: :open }, 0, :seller)
      status == :ok ? reply[:price] : nil
    end

    def collect_bids(ask, round)
      bids = {}
      @active = @active.select do |buyer|
        status, reply = invoke(buyer, { type: :ask, price: ask, round: round }, round, buyer.name)
        next false if status == :fault
        next false unless authorized_bid?(buyer.name, reply, round)

        bids[buyer.name] = reply[:price]
        true
      end
      bids
    end

    def ask_seller(bids, round)
      status, reply = invoke(@seller, { type: :bids, bids: bids, round: round }, round, :seller)
      status == :ok ? reply : nil
    end

    def settle(decision, round)
      status, receipt = @supervisor.guard(@settlement, { type: :settle, price: decision[:price] }) do |fault|
        @transcript.record(round: round, from: :settlement, message: fault)
      end
      return no_deal(:settlement_failed, round) if status == :fault

      @transcript.record(round: round, from: :settlement, message: receipt)
      { status: :deal, buyer: decision[:buyer], price: decision[:price], round: round, receipt: receipt }
    end

    def invoke(actor, message, round, name)
      status, reply = @supervisor.guard(actor, message) do |fault|
        @transcript.record(round: round, from: name, message: fault)
      end
      @transcript.record(round: round, from: name, message: reply) if status == :ok
      [status, reply]
    end

    def authorized_bid?(name, reply, round)
      return true if reply[:to] == :seller

      @transcript.record(round: round, from: :broker,
                         message: { type: :denied, actor: name, target: reply[:to] })
      false
    end

    def no_deal(reason, round)
      { status: :no_deal, reason: reason, round: round }
    end
  end

  # Renders the auction transcript and its outcome for the terminal.
  class Report
    def self.render(transcript, outcome)
      transcript.each { |entry| puts line(entry) }
      puts
      puts summary(outcome)
    end

    def self.line(entry)
      format("  r%<round>02d  %<from>-10s %<detail>s",
             round: entry[:round], from: entry[:from], detail: detail(entry[:message]))
    end

    def self.detail(message)
      case message[:type]
      when :ask then "asks $#{message[:price]}"
      when :bid then "bids $#{message[:price]}"
      when :accept then "AWARDS #{message[:buyer]} at $#{message[:price]}"
      when :denied then "DENIED  #{message[:actor]} -> #{message[:target]} (broker blocked the bid)"
      when :fault then fault_detail(message)
      when :receipt then "receipt #{message[:id]}  $#{message[:price]}  fee $#{message[:fee]}"
      end
    end

    def self.fault_detail(message)
      action = message[:giving_up] ? "gives up, drops out" : "supervisor restarts"
      "FAULT   #{message[:kind]}: #{message[:detail]} (attempt #{message[:attempt]}, #{action})"
    end

    def self.summary(outcome)
      case outcome[:status]
      when :deal then deal_summary(outcome)
      else "NO DEAL (#{outcome[:reason]})"
      end
    end

    def self.deal_summary(outcome)
      receipt = outcome[:receipt]
      "DEAL: #{outcome[:buyer]} wins at $#{outcome[:price]} (round #{outcome[:round]}) — " \
        "settled #{receipt[:id]}, fee $#{receipt[:fee]}, net $#{receipt[:net]}"
    end
  end

  # Wires the roster from CLI options, runs one auction, prints it, and —
  # under --replay — runs it a second time with the same seed to confirm the
  # transcript reproduces exactly.
  class Simulation
    ACTORS_DIR = File.join(__dir__, "actors")
    DEFAULT_BUYERS = %w[lowball_buyer jitter_buyer].freeze

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
      broker = Broker.new(cast: { seller: seller, buyers: buyers, settlement: settlement },
                          supervisor: Supervisor.new(max_restarts: @options[:restarts]),
                          max_rounds: @options[:rounds], transcript: transcript)
      [transcript, broker.run]
    end

    def verify_replay(first_transcript, first_outcome)
      transcript, outcome = play
      match = transcript.entries == first_transcript.entries && outcome == first_outcome
      puts
      puts match ? "REPLAY OK — the same seed reproduced the auction byte-for-byte" : "REPLAY MISMATCH"
    end

    def seller
      Actor.new(name: :seller, behavior_path: behavior("anchor_seller"),
                reservation: @options[:seller_floor], seed: @options[:seed])
    end

    def buyers
      scripts.each_with_index.map do |script, index|
        Actor.new(name: name_for(script), behavior_path: behavior(script),
                  reservation: @options[:buyer_max], seed: @options[:seed] + index + 1)
      end
    end

    def scripts
      (DEFAULT_BUYERS + @options[:buyers]).uniq
    end

    def name_for(script)
      script.sub(/_buyer$/, "").to_sym
    end

    def settlement
      Actor.new(name: :settlement, behavior_path: behavior("settlement"))
    end

    def behavior(name)
      File.join(ACTORS_DIR, "#{name}.rb")
    end
  end
end

ActorMesh::Simulation.new(options).run
