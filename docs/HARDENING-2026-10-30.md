# Release hardening — 30 October 2026

Baseline: `fbca19e` on `master`. This follow-up preserves protocol v5, SPAKE2 authentication and the existing session architecture. It does not declare the app production-ready.

## Confirmed fixes

- The receive deadline previously restarted on each read. One deadline now covers an entire frame, and EOF during a partial frame is an error rather than a clean disconnect. Paused-clock and duplex-stream regression tests cover both cases.
- Untrusted messages now use bounded fixed-width bincode decoding and reject trailing bytes. Valid v5 encodings are retained; no handshake identity, cipher or wire schema changed.
- A file overwrite previously counted existing bytes plus the write length, incorrectly rejecting writes that did not grow the file. The limit now uses the resulting file extent. A sparse-file boundary regression covers the case.
- The frontend accepted malformed bracketed IPv6 such as `[:::]:5899`. That regression was observed failing before the fix. IPv6 syntax and ambiguous IPv4 literals are now checked before connecting.
- Left and right physical modifiers can share one wire code without releasing the remote modifier prematurely. Punctuation and numpad Enter mappings were added to the existing key subset.
- Turning client input off now awaits held-key/button releases before disabling the local send permission. Unmounted viewers cannot apply a delayed permission change to a newer view.
- Disabled default input sessions do not initialize an OS injector. Disconnected host handles cannot reinitialize control, and cleanup clears the approval state.
- Session configuration diagnostics redact passwords.
- Production and QA macOS minimum versions now declare 10.15, matching the permission APIs and Tauri prerequisites.

The first attempt to run the new core regressions was blocked by an exhausted local disk. Only regenerable compiler caches and the failed audit tool's temporary compilation directory were removed. Source, preferences, installed apps and prior app backups were preserved. The checks below were subsequently executed successfully.

## Dependency audit

The official RustSec `cargo-audit` 0.22.2 macOS binary was downloaded from the RustSec release and its archive SHA-256 verified against GitHub's asset digest. Advisory results are dated observations, not an independent security audit.

| Lockfile | Baseline vulnerability advisories | After fixes |
| --- | ---: | ---: |
| Core | 5 | 0 |
| Tauri desktop | 4 | 0 |
| npm desktop tree | 3 affected packages | 0 |

Resolved Rust advisories: RUSTSEC-2024-0344, RUSTSEC-2026-0384, RUSTSEC-2026-0391, RUSTSEC-2026-0403 and RUSTSEC-2026-0398. Tokio is locked at 1.54.1; zstd/zstd-safe are 0.14.3/8.2.0. An unused x25519 dependency pulled in the old curve implementation and was removed. The public `crypto` feature name remains; actual SPAKE2/session encryption is always enabled.

Vite and its React plugin were upgraded to 7.3.7 and 5.2.0. Node.js 22.12+ is now required for the desktop toolchain. The audited npm tree includes development dependencies.

Warnings are not suppressed: bincode, zbus_names, zvariant and zvariant_derive are reported unmaintained. The desktop lockfile also reports proc-macro-error maintenance and glib unsoundness warnings. GTK/glib is a Linux dependency, absent from the macOS target graph; Linux desktop release readiness is not claimed. Retaining the v5 bincode schema requires continued review and a deliberate future serialization migration, rather than an unannounced protocol rewrite.

## Executed checks

| Check | Result |
| --- | --- |
| Core all-features tests | PASS: 47 library + 1 public integration; 1 permission-dependent test ignored |
| Core no-default-features tests | PASS: 45 library + 1 public integration |
| Format, core check, strict Clippy | PASS |
| Tauri format, tests, strict Clippy | PASS: 5 tests |
| Desktop frontend tests | PASS: 12 tests |
| TypeScript / Vite production build | PASS |
| Core release build and all-targets strict Clippy | PASS, including the opt-in QA example |
| npm audit including dev dependencies | PASS: 0 vulnerabilities |
| RustSec core / desktop | 0 vulnerability advisories; warnings retained above |
| Optimized signed production app + DMG build | PASS; notarization skipped because credentials are unavailable |
| Updated installed app | PASS: launches; Screen Recording and Accessibility still show Granted |
| Final DMG checksum and complete bundle signature | PASS |
| Hosted CI for `4436ffd` | BLOCKED before runner assignment; GitHub account billing/spending-limit restriction |

The reconnect regression now runs 32 cycles over actual TCP. Screen/input backends in those automated session tests are mocks, not actual OS validation. Three frontend tests validate release-policy classification with mock metadata; they do not claim actual notarization.

[GitHub run 40971352745](https://github.com/udhaybhat00/easycontrol/actions/runs/40971352745) failed without executing job steps. Its annotation states: “The job was not started because recent account payments have failed or your spending limit needs to be increased.” This is an account execution blocker, not an observed code-test failure. No billing settings or spending limits were changed. Previous baseline macOS runs passed, but those results do not substitute for executing the expanded pipeline at this revision.

## Distribution gate

Run from `desktop/`:

```bash
npm run verify:macos-release -- /Applications/EasyControl.app
```

Observed on the updated installed build:

| Gate | Result |
| --- | --- |
| Production bundle identity | PASS |
| macOS minimum version | PASS |
| Strict complete bundle signature | PASS |
| Developer ID Application signature | BLOCKED: only Apple Development identity available |
| Gatekeeper public distribution assessment | BLOCKED |
| Stapled notarization ticket | BLOCKED |

The gate fails closed. Developer signing does not qualify as a public release, and no Gatekeeper override was used. The installed app was updated with the same development identity and a recoverable copy of its predecessor was retained.

## Same-Mac OS input validation

`examples/macos_qa_client.rs` is an opt-in developer-only loopback client, not a bundled production capability. It uses the existing authenticated Session API, reads only the explicitly copied generated host password into memory, sends no input automatically, caps the number of test commands, and logs no passwords or screen contents. It has no keyboard listener, avoiding recursive input into a viewer.

The intended target is the unused host-side address field while the app is hosting, with no Enter or executable shortcut sent. The host still must approve input. Actual input testing is pending: macOS locked the screen and the desktop tool required a manual unlock before continuing. No real mouse/keyboard result is claimed here yet.

## Remaining release requirements

- Finish controlled OS input, permission-denial/revocation and cleanup observations after manual unlock and host approval.
- Validate actual viewer gestures/fullscreen and extended reliability; multi-monitor coverage requires another physical display.
- Supply Developer ID signing and notarization credentials through local secure configuration, then rerun the distribution gate and clean-machine installation checks.
- Resolve the GitHub Actions billing/spending-limit restriction and execute the expanded hosted pipeline; no Windows/Linux CI pass is claimed while it is blocked.
- Independently review cryptography, authorization and retained maintenance/unsoundness warnings before high-risk deployment.
- Treat Windows/Linux CI core checks as build/test evidence only, not desktop OS certification.

The macOS core is hardened further, but public production readiness remains blocked by these external and observed validation gates.
