# macOS stream-stall and recovery QA — 1 November 2026

Follow-up to [the real desktop/input report](QA-2026-11-01.md). This work uses the same real macOS host and existing Rust Session API. Protocol v5, authentication, cryptography and networking architecture are unchanged. The desktop remains a development preview.

## Tests and observed boundaries

| Test | Result | Evidence / limits |
| --- | --- | --- |
| Opaque traffic blackhole | PASS | Actual authenticated loopback connection closed about 27.005 seconds after encrypted bytes stopped reaching their peer; client returned to Home |
| Ordered TCP forwarding pause/resume | PASS | Current GUI recovered on the same authenticated connection after a 14.744-second pause |
| Frozen-stream indicator | PASS after fix | Actual viewer showed 0 FPS, 0.0 MiB/s IPC, a no-new-frames message and disabled input requests during the pause |
| Recovery indicator | PASS | After resume, warning cleared, checkbox became available and viewer reported 7 FPS with 2 native unread-frame replacements |
| Cancellation during stalled traffic | PASS through Rust API | Authenticated real client `SessionHandle.disconnect()` returned in 15 ms after the fixture confirmed PAUSED; process exited cleanly |
| GUI Disconnect click during stall | NOT VERIFIED | Native automation lost element/window handles and those attempts reached the timeout first; no successful button click is claimed |
| Test-process cleanup | PASS | Final clients and proxies exited; no 5899/15999 sockets remained |
| Installed application / permissions | PASS | Updated installed app launches; Screen Recording and Accessibility remain Granted |
| Timed view-only stability sample | PASS within stated scope | 300.04 seconds completed automatically; measured frame/resource counts below |
| Public distribution | BLOCKED | This Mac has Apple Development signing only, not Developer ID Application signing/notarization |
| Multi-monitor / separate-target viewer gestures | NOT TESTED | No additional physical display or independent input target used |

The first blackhole fixture discarded opaque bytes until timeout. The reusable fixture instead preserves every byte and uses backpressure while paused; it does not decrypt or modify frames. Both are controlled application-level TCP fault tests, not a claim of testing Wi-Fi packet loss, router failures, NAT traversal or another physical network.

No remote input was approved or injected in this follow-up. The same-Mac GUI stayed view-only. Real input/revocation evidence belongs to the preceding report.

## Confirmed viewer defect and fix

The prior viewer updated FPS/bandwidth only inside the frame-arrival branch. When frames stopped, it could indefinitely display the previous sample until the session timed out. It also lacked a distinct stale-frame state.

A metadata-only `FrameMeter` is now sampled by an independent one-second clock. With no frames, current FPS and IPC bandwidth fall to zero. After three seconds without a rendered frame (checked on the sampling tick), the viewer shows a stalled-stream message, pauses input forwarding, releases held client-side input and prevents enabling input requests. Fresh frames recover the indicator without granting host permissions. The clock and frame poll are cancelled when the viewer unmounts; no pixel data is retained by the meter.

Three regressions cover stalled sampling without another frame, recovery and fresh-session isolation. Actual GUI observation confirms the zero-FPS warning and recovery behavior. Frame age is measured on receipt/rendering, not capture time, so this does not measure end-to-end latency or detect every old frame queued upstream.

## Reproducible development fixture

`examples/loopback_fault_proxy.py` binds **only** `127.0.0.1:15999` and forwards to the real host on `127.0.0.1:5899`. It handles one session, holds at most one pending 64 KiB chunk per direction, uses TCP backpressure and has an explicit lifetime limit. It logs counters and timings only. It is never bundled with the application.

```bash
python3 -u examples/loopback_fault_proxy.py /tmp/easycontrol-unique-test.pause
```

Use a new, absent trigger filename. Start the actual EasyControl host on 5899, then connect the isolated QA GUI to `127.0.0.1:15999` with its real password and keep Send input unchecked. Creating the trigger file pauses forwarding; removing it resumes forwarding. Leave it paused to exercise timeout recovery. Finish by disconnecting and verifying both test ports are closed.

The existing development Rust QA client now accepts `--proxy`, restricted to this fixed loopback port. It reads only the explicitly copied generated password into memory and prints shutdown timing. No new production transport is introduced.

The initial fixture exposed a Python 3.13 shutdown-order issue: waiting for a server to close before cancelling its paused client handlers could leave the fixture alive. Its cleanup now cancels active handlers first, closes/aborts its owned writers with bounded waits, then awaits server closure. The stuck old fixture was explicitly identified and stopped; no unrelated process was terminated.

## Automated and build validation

- Core: 47 library + 1 public integration passed; 1 permission-dependent capture test ignored.
- Tauri bridge: 5 passed.
- Frontend: 18 passed, including 3 new frame-health regressions.
- Total: 71 distinct application tests passed; 1 ignored. Mock desktop tests remain distinct from actual OS evidence.
- Rust formatting, locked checks and all-target strict Clippy passed, including the development QA client.
- TypeScript/Vite production build passed.
- Development-signed production app + DMG and isolated QA app builds passed; strict app signature and DMG checksum verified.
- Last verified hosted baseline `aaebc47` passed: [run 41305455708](https://github.com/udhaybhat00/easycontrol/actions/runs/41305455708). This does not claim a later revision's hosted run has completed.

## Timed view-only soak

The automatically bounded view-only soak completed in **300.04 seconds**, with client exit code **0**. It used actual macOS capture and the optimized public Rust client; no input or image/pixel logs were produced.

- Received 1,209 frames, all sampled at 2880 × 1800; approximately **4.03 FPS** across this background/headless run.
- Decompressed RGBA bytes received: 25,069,824,000. This is not network throughput.
- Maximum reported client event lag: 0.
- Native host CPU snapshots: 11.5–81.6%; RSS 66,112–125,984 KiB.
- Client CPU snapshots: 1.0–55.1%; RSS 95,136–230,720 KiB.
- Client RSS included an early transient peak and then settled near 107 MiB; samples do not show monotonic growth, but do not prove absence of leaks. CPU percentages are per-process samples, not normalized whole-machine usage, and exclude WebKit/WindowServer.
- No sleep/scheduling gap exceeded the harness threshold. The client disconnected automatically and reported clean shutdown.

These variable rates are observations, not a promised minimum or a performance pass against an unstated target. The run is a short stability sample, not a multi-hour production soak or a viewer-rendering benchmark.

## Remaining release gates

- Developer ID signing, notarization and clean-machine installation.
- Real viewer input/focus tests on an independent controlled target, plus physical multi-monitor testing.
- Longer soak and broader physical network/OS coverage; a five-minute local sample is not production reliability certification.
- Independent security review and retained upstream dependency maintenance/unsoundness warnings described in the earlier hardening report.

No OS permission bypass, firewall change, cloud infrastructure or new production endpoint was introduced.


## Final cleanup

The viewer-health update is installed in `/Applications/EasyControl.app` using the existing development identity. Screen Recording and Accessibility remain granted. The production app is left stopped; all owned QA clients and proxies are closed and both test ports are unused. Production settings were preserved. QA-only history contains the actual local proxy test addresses.
