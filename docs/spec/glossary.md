# Glossary

kobako's ubiquitous language: one concept, one name. Every other specification is written in these words.

Concepts only. How a concept is spelled in Ruby or Rust belongs to the interface spec, and what it does to the behavior spec — both are written in this vocabulary rather than referenced from it.

Generated from `_data/glossary.yml` by `rake spec:generate`; edit that file.

| Term | Definition |
|------|------------|
| **Host App** | The application that embeds kobako. It holds every credential and policy decision, and chooses which of its own objects the guest may reach. |
| **Host Gem** | kobako itself — the side of the boundary that owns the guest, routes what it asks for, and decides what each outcome means. |
| **Guest Binary** | The compiled artifact untrusted code runs inside. It is the isolation boundary: nothing crosses it except as a message. |
| **Service** | A host object the guest reaches by name. It is the only route from guest code to a host resource. |
| **Wire Spec** | The contract every message crossing the boundary answers to. Each side implements it independently, so it is an agreement rather than a shared component. |
| **Transport** | The exchange of messages across the boundary. One Call is answered by one Reply, and both directions use that same pair. |
| **Envelope** | The part of a message that says where it goes and how it turned out. It is readable without a Codec, so routing and attribution never depend on one. |
| **Codec** | The agreement two endpoints reach about how values become bytes. It is replaceable: only the two endpoints need to share one. |
| **Outcome** | The final result of one Invocation. Every Invocation writes exactly one, whether it succeeded or failed. |
| **Panic** | The failed arm of an Outcome. It attributes the failure to a side of the boundary and carries the Error Record describing it. |
| **Fault** | The reason a Call is refused, returned to whoever issued it. It is kobako's own data rather than the caller's, so it rides the Envelope. |
| **Error Record** | A failure's name, message, and backtrace as one unit. Every channel that reports a guest failure carries this same shape. |
| **Run** | An Invocation that names an entrypoint already loaded in the guest, instead of supplying source to execute. |
| **Frame** | Setup data handed to the guest before an Invocation begins, so the guest starts knowing what this run was configured with. |
| **Catalog** | What a Sandbox has registered — the bindings and preloads fixed at setup, together with the Handle table each Invocation mints for itself. |
| **Invocation** | One run of guest code, from entry until it settles. It is the act; the Execution is the record it leaves. |
| **Execution** | The record one Invocation leaves — what it produced, what it wrote, and what it consumed. Frozen once the Invocation settles, and never revised. |

## Rejected names

| Term | Not | Why |
|------|-----|-----|
| Service | `adapter` | Names a translation role. What distinguishes a Service is that the guest can name it at all. |
