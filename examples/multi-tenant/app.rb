#!/usr/bin/env ruby
# frozen_string_literal: true

# Multi-tenant demo: ONE Sandbox serves every tenant concurrently, and each
# invocation names the tenant it acts for.
#
# The lesson this example encodes
# -------------------------------
# A Sandbox holds no state from any run, so Threads may invoke a single
# shared one concurrently (B-22). What each invocation still needs is its
# own identity, and that is what the two setup-time-vs-run-time binding
# forms give you:
#
#   * `bind("Tenant::Store")` with no object declares a FILLABLE path — the
#     guest sees the constant, the host defers the object (B-62). An
#     invocation that never fills it fails closed as a ServiceError.
#   * `eval(source) { |ctx| ctx.bind(path, object) }` fills it for that one
#     invocation (B-63), so the object is never shared with another Thread.
#
# The guest source is IDENTICAL for every tenant. Nothing in it names a
# tenant; the host decides whose ledger the run reaches by what it binds.
#
# The obligation sharing adds
# ---------------------------
# A statically bound object is reached by every Thread at once, so it must
# be thread-safe itself — `Audit` here guards its own state with a Mutex.
# An object supplied per invocation carries no such requirement, which is
# the practical reason to prefer `ctx.bind` for anything mutable.
#
# `gvl: :release` lets the Threads run their guest code in parallel instead
# of serialising on Ruby's GVL (B-64). It changes scheduling only — the
# isolation this demo prints is identical under the default `:hold`.
#
# Usage:
#   ruby examples/multi-tenant/app.rb
#
# From a clone of the kobako repository, prefix with `bundle exec` so the
# local checkout is used instead of the released gem.

require "bundler/inline"

gemfile do
  source "https://rubygems.org"
  gem "kobako", "~> 0.20.0"
end

require "kobako"

# Example types are nested under MultiTenant so the file carries a single
# top-level constant and reads top-down.
module MultiTenant
  # One tenant's book of entries. It has no wire representation, so it
  # crosses into the guest as a Capability Handle minted by the invocation
  # that asked for it (B-14) and dies with that invocation.
  class Ledger
    attr_reader :name, :entries

    def initialize(name, entries)
      @name = name
      @entries = entries
    end
  end

  # The per-invocation Service: it hands out exactly one tenant's Ledger.
  # A fresh Store is bound per run through `ctx.bind`, so no Thread can
  # reach another tenant's book even though every Thread shares the Sandbox.
  class Store
    def initialize(ledger)
      @ledger = ledger
    end

    attr_reader :ledger
  end

  # The statically bound Service: one object, reached by every Thread at
  # once. Sharing is what makes the Mutex mandatory — kobako isolates
  # invocations from each other, not a host object from itself.
  class Audit
    def initialize
      @mutex = Mutex.new
      @seen = []
    end

    def record(tenant)
      @mutex.synchronize { @seen << tenant }
      nil
    end

    def seen = @mutex.synchronize { @seen.dup }
  end

  # The guest program every tenant runs. It names no tenant: the ledger it
  # reaches is whatever the host bound for this one invocation.
  REPORT = <<~'MRUBY'
    ledger = Tenant::Store.ledger
    Audit.record(ledger.name)

    total = 0
    ledger.entries.each { |cents| total += cents }
    puts "#{ledger.name}: #{ledger.entries.size} entries"

    { "tenant" => ledger.name, "total" => total }
  MRUBY

  LEDGERS = {
    "acme" => [1200, 450, 8000],
    "globex" => [99, 99],
    "initech" => [50_000, 2500, 750, 300]
  }.freeze

  # Runs one tenant's report on the SHARED Sandbox, filling the fillable
  # path for this invocation only. The returned Execution carries this
  # run's value and its own captured stdout — neither is stored on the
  # Sandbox, so a concurrent run cannot observe either.
  def self.report(sandbox, tenant)
    ledger = Ledger.new(tenant, LEDGERS.fetch(tenant))
    sandbox.eval(REPORT) { |ctx| ctx.bind("Tenant::Store", Store.new(ledger)) }
  end

  # Fans every tenant out onto its own Thread against the one Sandbox and
  # collects each run's Execution.
  def self.report_all(sandbox, tenants)
    tenants.map { |tenant| Thread.new { [tenant, report(sandbox, tenant)] } }
           .map(&:value)
  end
end

audit = MultiTenant::Audit.new

sandbox = Kobako::Sandbox.new(gvl: :release)
sandbox.bind("Audit", audit)          # static: shared by every Thread
sandbox.bind("Tenant::Store")         # fillable: supplied per invocation

results = MultiTenant.report_all(sandbox, MultiTenant::LEDGERS.keys)

# Exercise the fillable's closed default so running the example proves it:
# an invocation that binds nothing must be refused, not served someone
# else's leftovers.
unfilled =
  begin
    sandbox.eval("Tenant::Store.ledger")
    "NOT refused — the fillable is broken!"
  rescue Kobako::ServiceError
    "refused"
  end

puts "multi-tenant demo — one Sandbox, one identity per invocation"
puts
puts "concurrent runs on the SHARED Sandbox (#{results.size} Threads, gvl: :release):"
results.each do |tenant, execution|
  # Both observables must be this tenant's: the value proves the Handle it
  # minted stayed in this run, the capture proves the output did.
  own = execution.value["total"] == MultiTenant::LEDGERS.fetch(tenant).sum &&
        execution.stdout == "#{tenant}: #{MultiTenant::LEDGERS.fetch(tenant).size} entries\n"
  verdict = own ? "own ledger only" : "LEAKED across tenants!"
  puts format("  %<tenant>-8s total %<total>-7d %<verdict>s",
              tenant: tenant, total: execution.value["total"], verdict: verdict)
  puts "           stdout #{execution.stdout.inspect}"
end
puts
puts "unfilled invocation — nothing bound for this run:"
puts "  Tenant::Store.ledger : #{unfilled}   # fails closed (Kobako::Unresolved)"
puts
puts "statically bound Audit — one object, every Thread:"
puts "  recorded #{audit.seen.sort.inspect}   # its own Mutex, not kobako's isolation"
