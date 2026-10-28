# Security policy

EasyControl is development software. No version is currently designated as a supported, production-hardened security release. Do not rely on it for critical or unattended access until the documented validation and review gates are complete.

## Report a vulnerability

Contact [Udhay Bhat](https://github.com/udhaybhat00) to arrange a private reporting channel. If GitHub private vulnerability reporting is available to you for this repository, use its **Report a vulnerability** action. Do not publish a working exploit in a public issue before coordinating with the maintainer.

Include the affected revision, platform, reproduction steps, impact and a minimal proof of concept. Do not include real session passwords, session keys, private screen captures or clipboard contents. No response-time or bounty commitment is made.

## Current boundaries

- SPAKE2 authentication and encrypted confirmation are implemented; protocol v5 session traffic is encrypted.
- Desktop hosts start with remote input denied and enforce permission decisions locally.
- The client cannot grant itself host permissions.
- Native frame delivery is bounded and scoped to the active authenticated session.
- Session passwords are not persisted by the desktop UI.
- OS Screen Recording and Accessibility controls remain authoritative.

These mechanisms are not a substitute for an independent cryptographic/security audit. Actual input denial, prolonged two-machine reliability, distribution signing and clean-machine validation remain release gates. See the [QA report](docs/QA-2026-10-28.md) for observed evidence and limitations.
