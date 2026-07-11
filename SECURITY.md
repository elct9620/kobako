# Security Policy

kobako runs untrusted guest code inside an in-process Wasm sandbox, so a break in
its isolation boundary is treated as a security issue. This file is about **reporting
such an issue**; for how the boundary is meant to work and where your
responsibilities as a host begin, see [`docs/security-model.md`](docs/security-model.md).

## Supported versions

kobako is pre-1.0. Security fixes land on the latest released `0.x` version only;
upgrade to it before reporting.

## Reporting a vulnerability

Report privately through GitHub's **[Report a vulnerability](https://github.com/elct9620/kobako/security/advisories/new)**
flow — please do not open a public issue or pull request for a suspected vulnerability.

Include the affected version, a minimal guest script or host setup that reproduces the
issue, and what boundary you expected to hold. You can expect an initial acknowledgement
within a few days; once a fix or mitigation is agreed, disclosure is coordinated through
a GitHub Security Advisory. Reporters are credited in the published advisory unless you
ask to stay anonymous.

## Scope

In scope is anything that lets guest code cross the isolation boundary it should not:
reaching host memory, the filesystem, the network, or `ENV`; obtaining ambient time or
entropy the host froze; reaching a Service you never bound; or a
memory-safety fault in the host codec or wasmtime driver.

Out of scope is what a bound Service is *designed* to expose: if guest code reaches a
method because you bound an object carrying it, that is a host-side authorization
choice, not a sandbox escape — narrow the bound surface as described in the security
model. Resource exhaustion that stays within the limits you configured is likewise
expected behaviour, not a vulnerability.
