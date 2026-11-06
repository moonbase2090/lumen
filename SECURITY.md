# Security Policy

Please do not report security vulnerabilities through public GitHub issues, discussions, pull requests or commit messages.

## Report privately

Use either channel:

1. **GitHub private vulnerability reporting.** Open the **Security** tab of this repository and choose **Report a vulnerability**.
2. **Email** [security@lumenprotocol.org](mailto:security@lumenprotocol.org). The address is also listed in [`/.well-known/security.txt`](https://lumenprotocol.org/.well-known/security.txt).

Include the affected component, the spec version or revision, reproduction steps and the impact. Reports are triaged and fixed privately, and disclosed once a fix is available, as described below.

## Scope

The [threat model](spec/v0/threat-model.md) states what is in and out of scope. The [parity rule](spec/v0/threat-model.md#threat.parity) is the test for whether a Lumen asset gives more than the PTY-write path or the user's own dotfiles already give.

## Severity

Findings are graded against the fixed severity rubric used for the specification's independent reviews, as described in [Independent review](spec/v0/threat-model.md#threat.review). The rubric is not part of the public specification.

## What happens to a report

1. **Acknowledgement.** We acknowledge a report within 3–5 business days. If you hear nothing, write again to the email address: a missed message is more likely than silence.
2. **Triage.** Within 7 days we reproduce the report, decide whether it is in scope, and give it a severity. We apply the [parity rule](spec/v0/threat-model.md#threat.parity): a report that shows a Lumen asset giving more than a PTY write or the user's own dotfiles already give is a vulnerability; one that does not is not, and we say why. We tell you the outcome and the severity.
3. **Fix.** We aim to have a fix ready within these targets, counted from the end of triage:

   | Severity | Fix target |
   | --- | --- |
   | Critical | 14 days |
   | High | 30 days |
   | Medium | 90 days |
   | Low | The next regular release |

   A target is a goal, not a promise. If we will miss it, we tell you and say why.
4. **Release and advisory.** The fix ships in a patched release, and we publish a security advisory for it (see below).

A crash, an out-of-limit decode or a memory-error report found in the image decoder by any test blocks the release until a patched build ships. This is the [decoder gate](spec/v0/conformance.md#gate.decoder) and the [fuzzing rules](spec/v0/conformance.md#gate.fuzzing), and it applies to reports as well as to our own tests.

## Embargo and disclosure

We ask you to keep the details private until the advisory is published. We disclose when the fix is released, or 90 days after your report, whichever comes first. The 90 days are extended only if you agree to a later date; if we ask for more time and you do not agree, the 90 days stand. If you believe a report is already being exploited, say so in the first message and we will move it to the front.

If another project is affected, we tell its maintainers privately and agree the date with them. We do not ask you for a non-disclosure agreement.

## Advisories

When a fix is released we publish a GitHub security advisory for the repository. It names the affected and fixed versions, the severity, the impact and a workaround if there is one, and credits the reporter by the name they choose, or leaves them out if they prefer. We request a CVE identifier for anything graded Medium or higher. Advisories describe the fix and its effect; exploit details beyond what is needed to understand and mitigate the problem are not published.
