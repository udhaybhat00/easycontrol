# Changelog

All entries below are unreleased development work. There is no public signed installer release yet.

## Unreleased

### Release hardening — 30 October 2026

- Update affected Tokio/zstd dependencies, remove unused legacy curve dependency, and upgrade the Vite toolchain; current locked dependency audits report zero vulnerability advisories, with upstream warnings documented.
- Bound message decoding and whole-frame deadlines; reject truncated frames and trailing bytes while preserving protocol v5.
- Fix file-size accounting for overwrites and malformed numeric address validation.
- Correct shared modifier release, add punctuation mapping, and drain held inputs before disabling client input.
- Redact configuration passwords and prevent closed sessions from reinitializing input backends.
- Add a fail-closed macOS distribution gate and opt-in headless same-Mac QA client that is not bundled with the app.
- Expand CI with Windows/Linux headless core checks and locked dependency audits.

See [release hardening evidence](docs/HARDENING-2026-10-30.md). Public distribution remains blocked by Developer ID/notarization and outstanding real-device validation.

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
