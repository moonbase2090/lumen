# Security Policy

Please do not report security vulnerabilities through public GitHub issues, discussions, pull requests or commit messages.

## Report privately

Use either channel:

1. **GitHub private vulnerability reporting.** Open the **Security** tab of this repository and choose **Report a vulnerability**.
2. **Email** [security@lumenprotocol.org](mailto:security@lumenprotocol.org). The address is also listed in [`/.well-known/security.txt`](https://lumenprotocol.org/.well-known/security.txt).

Include the affected component, the spec version or revision, reproduction steps and the impact. Reports are triaged and fixed internally, and disclosed once a fix is available.

## Scope

The [threat model](spec/v0/threat-model.md) states what is in and out of scope. The [parity rule](spec/v0/threat-model.md#threat.parity) is the test for whether a Lumen asset gives more than the PTY-write path or the user's own dotfiles already give.

## Severity

Findings are graded against the fixed severity rubric used for the specification's independent reviews, as described in [Independent review](spec/v0/threat-model.md#threat.review). The rubric is not part of the public specification.
