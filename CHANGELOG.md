# Changelog

All entries below are unreleased development work. There is no public signed installer release yet.

## Unreleased

### Desktop reliability and real macOS QA

- Retain the host listener after rejected authentication; support a correct-password retry without restarting it.
- Serialize desktop lifecycle operations and cancel pending connections on disconnect.
- Restore visible host approval/revocation and macOS permission status.
- Start desktop hosts view-only and scope frame/input/monitor commands to the current authenticated session.
- Bound frame delivery/decompression, handle padded RGBA rows and correct Retina coordinate mapping.
- Release held input on teardown/revocation; coalesce movement and bound wheel requests.
- Permit native binary IPC in production and QA content policies.
- Observe actual macOS screen rendering through the isolated QA client; retain explicit limits on unperformed input tests.
- Include MIT licensing, regression coverage and macOS validation workflow.

### Documentation and website

- Expand the README with actual idle-state screenshots, architecture, repository tree, setup, permissions, validation and release boundaries.
- Add contribution and security-reporting guidance.
- Add a responsive graphite/lavender website with documentation, real screenshots, themes, keyboard navigation and GitHub Pages deployment.

See [QA — 28 October 2026](docs/QA-2026-10-28.md) for detailed test evidence. No Windows/Linux or production-readiness certification is implied.
