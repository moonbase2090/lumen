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

## What happens to a report

1. **Acknowledgement** within 3 business days.
2. **Triage** within 7 days: we reproduce the report, decide whether it is in scope, give it a severity and tell you the outcome. The test for "is this a vulnerability" is the spec's [parity rule](@/spec/v0/threat-model.md#threat.parity): does a Lumen asset give more than a PTY write, or the user's own dotfiles, already give?
3. **Fix** within a target counted from the end of triage:

| Severity | Fix target |
|---|---|
| Critical | 14 days |
| High | 30 days |
| Medium | 90 days |
| Low | The next regular release |

A target is a goal, not a promise. If we will miss it, we tell you and say why.

4. **Release and advisory.** The fix ships in a patched release and we publish a security advisory.

A crash, an out-of-limit decode or a memory-error report found in the image decoder by any test blocks the release until a patched build ships ([decoder gate](@/spec/v0/conformance.md#gate.decoder), [fuzzing](@/spec/v0/conformance.md#gate.fuzzing)).

## Embargo and advisories

Please keep the details private until the advisory is published. We disclose when the fix is released, or 90 days after your report, whichever comes first. The 90 days are extended only if you agree to a later date; if we ask for more time and you do not agree, the 90 days stand. If you believe a report is already being exploited, say so in the first message and we will move it to the front.

If another project is affected, we tell its maintainers privately and agree the date with them. We do not ask you for a non-disclosure agreement.

The advisory names the affected and fixed versions, the severity, the impact and any workaround, and credits you by the name you choose, or leaves you out if you prefer. We request a CVE identifier for anything graded Medium or higher. Advisories describe the fix and its effect; exploit details beyond what is needed to understand and mitigate the problem are not published.
