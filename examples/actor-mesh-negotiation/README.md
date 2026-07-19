# Actor-Mesh Negotiation

A buyer, a seller, and a settlement actor — each an untrusted third-party script in its own `Kobako::Sandbox` — reach a price through a host that is the only path between them. The host owns the whole mesh runtime; each actor owns nothing but the behavior it was written to run.

This is the demonstration of composing *mutually distrusting* actors. Because every actor is one sandbox, a crash, a forged move, or a peek at a rival's private data is contained by construction: an actor sees only the message the host delivered and the capabilities the host bound to its own sandbox — never the counterparty's reservation price, memory, or code. It is the multi-actor counterpart to the single-script examples ([codemode](../codemode/README.md), [serverless](../serverless/README.md), [plugin-rs](../plugin-rs/README.md)): those run one untrusted script, while this one wires several into a supervised topology where the host is the broker.

## The shape, and why it has to be this shape

```
                    ┌─ host (the only path between actors) ─────────────┐
                    │  Broker · authorization matrix · Supervisor ·     │
                    │  Transcript / replay                              │
                    └──┬───────────────┬──────────────────┬────────────┘
              #run(:Behavior, msg)     │                  │ host-only
                       │               │                  │
                ┌──────┴─────┐  ┌──────┴─────┐    ┌────────┴─────┐
                │ Sandbox    │  │ Sandbox    │    │ Sandbox      │
                │ buyer      │  │ seller     │    │ settlement   │
                │ Wallet     │  │ Wallet     │    │ (no Wallet)  │
                │ Memory Dice│  │ Memory Dice│    │ Memory       │
                └────────────┘  └────────────┘    └──────────────┘
```

Buyer and seller exchange offers only through the broker; neither can address settlement — the host invokes it once, after both agree. The broker enforces this with an authorization matrix, so a reply aimed at a peer it was not permitted to reach is blocked and recorded rather than delivered.

An actor's private reservation price is a `Wallet` the host binds to that one sandbox; the counterparty's sandbox never binds it, so the number cannot leak across the mesh. This is the whole reason the compute runs in a sandbox rather than as a plain Ruby object: the actors come from mutually distrusting sources, and isolation — not intelligence — is what the sandbox provides.

The guest VM is discarded after every turn (a fresh `mrb_state` per `#run`), so an actor cannot hold state in the sandbox. That is not a limitation to work around — it is the mesh's foundation. An actor's identity, private `Memory`, reservation, and randomness all live host-side and outlive the sandbox instance, so the sandbox is disposable compute the host can recreate at any time with no state loss. The same property is what makes the model Ractor-ready (below).

## Running

The script uses `bundler/inline`, so it resolves its own dependencies on first run — no `Gemfile` is required in the working directory.

```bash
ruby examples/actor-mesh-negotiation/app.rb                                    # a deal
ruby examples/actor-mesh-negotiation/app.rb --buyer-max 700 --seller-floor 800 # no overlap, no deal
ruby examples/actor-mesh-negotiation/app.rb --with-cheater                     # broker blocks a forged move
ruby examples/actor-mesh-negotiation/app.rb --with-faulty                      # an actor crashes and forfeits
ruby examples/actor-mesh-negotiation/app.rb --with-jitter --replay             # randomized, yet reproducible
```

From a clone of the kobako repository, prefix with `bundle exec` so the local checkout is used. CLI parsing runs before `bundler/inline` resolves the inline Gemfile.

## Configuration

| Flag             | Purpose                                                              | Default |
|------------------|---------------------------------------------------------------------|---------|
| `--buyer-max N`  | Buyer's private ceiling, injected as its reservation price.         | `1000`  |
| `--seller-floor N` | Seller's private floor, injected as its reservation price.        | `800`   |
| `--rounds N`     | Message budget before the talk breaks off with no deal.             | `20`    |
| `--seed N`       | Seed for the per-actor Dice RNG; the only source of randomness.     | `1`     |
| `--with-cheater` | Swap in a buyer that addresses settlement directly; the broker blocks it. | off |
| `--with-faulty`  | Swap in a buyer that raises on its turn; the supervisor makes it forfeit.  | off |
| `--with-jitter`  | Swap in a buyer whose bids jitter through the seeded Dice.          | off     |
| `--replay`       | Run twice with the same seed and confirm the transcript reproduces exactly. | off |

## What each run shows

| Property                | Try                       | What you see                                                                 |
|-------------------------|---------------------------|-----------------------------------------------------------------------------|
| Host-brokered mesh      | (default)                 | every offer flows host → actor → host; the two actors never touch directly. |
| Mutual distrust         | `--with-cheater`          | the buyer aims a forged `accept` at settlement; the broker records `DENIED` and no forged deal reaches settlement. |
| Let-it-crash            | `--with-faulty`           | the buyer raises; the supervisor records the fault, the buyer forfeits, and the host keeps running. |
| Deterministic replay    | `--with-jitter --replay`  | the buyer's bids are randomized, yet the re-run reproduces the transcript byte-for-byte. |

Replay is verified by re-executing the negotiation with the same seed and comparing against the recorded transcript, so it re-covers the broker's routing as well as each actor's play. Determinism holds because the sandbox is hermetic (no ambient clock or entropy) and every source of variation — reservations, seed, message order — is host-owned; the seeded `Dice` is the actor's only randomness.

## Writing your own actor

An actor is a file under `actors/` that defines a `Behavior` with a `self.call(msg)` entrypoint. It receives the incoming message as a Hash and returns its reply as a Hash addressed with `to:`:

```ruby
class Behavior
  def self.call(msg)
    floor = Wallet.reservation                 # this actor's private reservation
    bid = msg[:price]
    return { to: :buyer, type: :accept, price: bid } if bid && bid >= floor

    { to: :buyer, type: :counter, price: floor }
  end
end
```

The message types are `:open` (the host's kickoff to the seller), `:counter` (a price on the table), and `:accept` / `:reject` (terminal). An actor may read only the capabilities the host bound to it — `Wallet` (its reservation), `Memory` (private scratch that survives across turns, since the VM does not), and `Dice` (seeded randomness). Anything else raises host-side, and any `to:` the authorization matrix forbids is blocked. Point `--with-jitter`'s slot (or the `buyer_script` switch in `app.rb`) at your file to run it.

## Today it is serial; the design is Ractor-ready

Every actor runs in the main Ractor, and the wasm segment runs under the GVL, so the mesh is cooperatively serial within one process — fine for a message-driven negotiation, and it makes replay clean. True parallelism would let each actor run in its own Ractor, and the mesh is already the right shape for it: actors share nothing, and every message is a plain wire value (a Hash of symbols and integers), which is exactly the copyable form a Ractor requires to move a message between Ractors.

It does not work *today* for one concrete reason. The kobako native extension has not declared itself Ractor-safe, so constructing a `Kobako::Sandbox` in a non-main Ractor raises `Ractor::UnsafeError` at `Kobako::Runtime.from_path`. Lifting that is a bounded, host-side change — the extension opts into `rb_ext_ractor_safe` and confines each `Runtime` to one Ractor — after which the upgrade here is purely orchestration: the broker becomes a Ractor that owns the transcript and routes copied messages, and each actor runs its sandbox in its own Ractor. The actor contract, the authorization matrix, and the transcript do not change. Because all state is host-side, an actor is never migrated — only its disposable sandbox moves.

This example is the companion to the single-script demos: [codemode](../codemode/README.md) and [plugin-rs](../plugin-rs/README.md) run one untrusted script against host capabilities; here many untrusted actors are composed under a host that brokers and supervises them.
