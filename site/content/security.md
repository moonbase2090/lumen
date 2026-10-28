+++
title = "Security & disclosure"
+++

Security is Lumen's prime directive. Please report vulnerabilities privately, never in public issues.

**Do not open a public issue for a vulnerability.** Use one of the private channels below.

## Report privately

1. **GitHub private vulnerability reporting** on [github.com/moonbase2090/lumen](https://github.com/moonbase2090/lumen/security/advisories/new) (Security tab, then *Report a vulnerability*).
2. **Email** [security@lumenprotocol.org](mailto:security@lumenprotocol.org). The address is also listed in [`/.well-known/security.txt`](/.well-known/security.txt).
3. Include the affected component, the spec version or revision, reproduction steps and the impact.

## Scope

| In scope | Examples |
|---|---|
| Lumen specification | Clauses that permit unsafe behaviour |
| Published crates | Protocol libraries |
| Reference implementations | Behaviour that diverges from the spec's security requirements |

The spec's [threat model](@/spec/v0/threat-model.md) explains what is in and out of scope, and every chapter that carries risk links to it.
