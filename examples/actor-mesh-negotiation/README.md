# Actor-Mesh Negotiation

One seller auctions an item to several competing buyers, with a settlement actor to close the deal — and every actor is an untrusted third-party script in its own `Kobako::Sandbox`, reaching the others only through a host that is the single path between them. The host owns the whole mesh runtime; each actor owns nothing but the behavior it was written to run.

This is the demonstration of composing *many mutually distrusting* actors. Because every actor is one sandbox, a crash, a forged move, or a peek at a rival's private data is contained by construction: an actor sees only the message the host delivered and the capabilities the host bound to its own sandbox — never a rival's reservation price, memory, or code. It is the multi-actor counterpart to the single-script examples ([codemode](../codemode/README.md), [serverless](../serverless/README.md), [plugin-rs](../plugin-rs/README.md)): those run one untrusted script, while this one wires several into a supervised topology where the host brokers every exchange.

## The shape, and why it has to be this shape

```
                 ┌─ host broker ─────────────────────────────────────┐
                 │  broadcast ask · collect sealed bids · award ·     │
                 │  authorization · supervisor · transcript / replay  │
                 └──┬───────────┬───────────┬────────────┬───────────┘
            ask / bid            │           │            │ host-only
                 │               │           │            │
           ┌─────┴────┐   ┌──────┴───┐ ┌─────┴────┐ ┌─────┴───────┐
           │ seller   │   │ buyer    │ │ buyer    │ │ settlement  │
           │ anchor   │   │ lowball  │ │ jitter   │ │ (no Wallet) │
           └──────────┘   └──────────┘ └──────────┘ └─────────────┘
```

Each round the broker broadcasts the seller's ask, collects one sealed bid per buyer, and hands them to the seller, who either awards the sale to the highest bid that meets its ask or concedes a little. Buyers bid only to the seller and never see each other's bids — the broker relays only the seller's public ask — so the competition runs entirely through the host. A bid aimed anywhere else is blocked: the authorization matrix lets a buyer address only the seller, and settlement is reachable by the host alone, after the seller awards.

An actor's private reservation price is a `Wallet` the host binds to that one sandbox; a rival's sandbox never binds it, so the number cannot leak across the mesh. This is the whole reason the compute runs in a sandbox rather than as a plain Ruby object: the actors come from mutually distrusting sources, and isolation — not intelligence — is what the sandbox provides.

The guest VM is discarded after every turn (a fresh `mrb_state` per `#run`), so an actor cannot hold state in the sandbox. That is not a limitation to work around — it is the mesh's foundation. An actor's identity, private `Memory`, reservation, and randomness all live host-side and outlive the sandbox instance, so the sandbox is disposable compute the host can recreate at any time with no state loss.

## Running

The script uses `bundler/inline`, so it resolves its own dependencies on first run — no `Gemfile` is required in the working directory. Two well-behaved buyers always compete; each `--with-<type>` adds one more, misbehaving buyer on top.

```bash
ruby examples/actor-mesh-negotiation/app.rb                 # two buyers compete for the item
ruby examples/actor-mesh-negotiation/app.rb --with-flaky    # +1 buyer that crashes once and recovers
ruby examples/actor-mesh-negotiation/app.rb --with-faulty   # +1 buyer that keeps crashing and drops out
ruby examples/actor-mesh-negotiation/app.rb --with-cheater  # +1 buyer the broker blocks
ruby examples/actor-mesh-negotiation/app.rb --seed 7        # a different Dice stream — a different winner
ruby examples/actor-mesh-negotiation/app.rb --replay        # run twice, reproduce byte-for-byte
```

From a clone of the kobako repository, prefix with `bundle exec` so the local checkout is used. CLI parsing runs before `bundler/inline` resolves the inline Gemfile.

## Configuration

| Flag             | Purpose                                                              | Default |
|------------------|---------------------------------------------------------------------|---------|
| `--buyer-max N`  | Every buyer's private ceiling, injected as its reservation price.   | `1000`  |
| `--seller-floor N` | The seller's private floor, injected as its reservation price.    | `800`   |
| `--rounds N`     | Auction rounds before it breaks off with no deal.                   | `20`    |
| `--seed N`       | Seed for the Dice RNG the jitter buyer draws from; it changes who wins. | `1`  |
| `--restarts N`   | Supervisor restart budget for a faulting turn.                      | `2`     |
| `--with-flaky`   | Add a buyer that faults once then recovers on restart.              | off     |
| `--with-faulty`  | Add a buyer that keeps crashing until it drops out.                 | off     |
| `--with-cheater` | Add a buyer that bids at settlement directly; the broker blocks it. | off     |
| `--replay`       | Run twice with the same seed and confirm the transcript reproduces. | off     |

The three `--with-*` flags stack, so `--with-faulty --with-cheater` runs four buyers — two of them misbehaving — while the two well-behaved buyers still reach a deal.

## What each run shows

| Property                | Try              | What you see                                                                 |
|-------------------------|------------------|-----------------------------------------------------------------------------|
| Competing mesh          | (default)        | two buyers bid each round; the seller awards the highest bid that has reached its descending ask. |
| Mutual distrust         | (default)        | buyers never see each other's bids or the seller's floor — the broker relays only the public ask. |
| Let-it-crash (recover)  | `--with-flaky`   | a buyer faults once; the supervisor restarts it, the retry recovers, and it competes on. |
| Let-it-crash (drop)     | `--with-faulty`  | a buyer keeps crashing; the supervisor drops it after the budget, and the auction still reaches a deal — the mesh survives one actor's crash. |
| Capability broker       | `--with-cheater` | a buyer aims its bid at settlement; the broker denies it and the buyer drops, so no forged deal reaches settlement. |
| Deterministic replay    | `--replay`       | the jittered auction re-runs and reproduces the transcript byte-for-byte. |

Replay is verified by re-executing the auction with the same seed and comparing against the recorded transcript, so it re-covers the broker's routing as well as each actor's play. Determinism holds because the sandbox is hermetic (no ambient clock or entropy) and every source of variation — reservations, seed, bid order — is host-owned; the seeded `Dice` is a buyer's only randomness.

## Writing your own actor

An actor is a file under `actors/` that defines a `Behavior` with a `self.call(msg)` entrypoint, returning its reply as a Hash. A buyer receives the seller's ask and replies with a bid addressed to the seller:

```ruby
class Behavior
  def self.call(_msg)
    ceiling = Wallet.reservation                        # this buyer's private ceiling
    bid = [ceiling, (Memory.get("bid") || 500) + 60].min # raise toward the ceiling each round
    Memory.set("bid", bid)
    { to: :seller, type: :bid, price: bid }
  end
end
```

The seller is the auctioneer: on `{type: :open}` it returns its opening ask, and on `{type: :bids, bids: {name => price}}` it returns either `{type: :accept, buyer:, price:}` to award the sale or `{type: :ask, price:}` to concede. An actor may read only the capabilities the host bound to it — `Wallet` (its reservation), `Memory` (private scratch that survives across turns, since the VM does not), and `Dice` (seeded randomness). Anything else raises host-side, and any `to:` the authorization matrix forbids is blocked. Add your buyer to `DEFAULT_BUYERS` (or a `--with-*` slot) in `app.rb` to run it.

This example is the companion to the single-script demos: [codemode](../codemode/README.md) and [plugin-rs](../plugin-rs/README.md) run one untrusted script against host capabilities; here many untrusted actors are composed under a host that brokers and supervises them.
