# Guides

These guides cover what a Host App or an implementer relies on, past the
features the project README introduces.

## Host App

Pick a guide by what you want the guest to do. Each ends with a Rust SDK
section mapping the Ruby calls to the SDK's.

| If you want to | Read |
|---|---|
| let guest code call into your host | [Services](services.md) |
| read what a run produced, or why it failed | [Invocations](invocations.md) |
| bound what a run may consume | [Resource Limits](limits.md) |
| run many invocations at once | [Concurrency](concurrency.md) |
| let guest code read and write files | [Mounts](mounts.md) |
| ship guest code with the Sandbox | [Snippets](snippets.md), [Extensions](extensions.md) |
| give the guest Regexp or JSON | [Guest Binary Variants](variants.md) |
| decide what the guest may reach | [Security Model](security.md) |

## Implementer

Pick a guide by how much of kobako you replace.

| If you want to | Read |
|---|---|
| see which parts you own at each level | [Assembly Levels](assembly-levels.md) |
| build your own Guest Binary, Codec, or Engine | [Customization](customization.md) |
