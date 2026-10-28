# EasyControl

EasyControl is an open-source, direct remote-desktop application built with Rust, Tauri 2, React, and TypeScript. It is designed for authenticated LAN sessions: a host shares an address and one-time session password, and a client connects directly without a cloud device-ID service.

> **Project status:** active development. The desktop application builds on macOS and the core has automated coverage, but it is not yet production-ready. See [Current limitations](#current-limitations) before relying on it for unattended or critical access.

## What it does

- Direct host-to-client connections using `host:port`
- Password-authenticated sessions using SPAKE2 with encrypted key confirmation
- Protocol v5 with encrypted session traffic
- Screen capture, frame delivery, and remote input backends behind Rust feature flags
- Host-controlled permissions for remote input, clipboard, file transfer, and chat
- A Tauri desktop application with light, dark, and system appearance modes
- Local recent-connection and saved-device lists; passwords are never persisted by the desktop UI
- A separate **EasyControl QA** app identity for same-Mac host/client testing without overwriting production desktop preferences

## Screenshots and interface

The desktop app provides a compact native-style workspace for:

- starting or stopping a local host session;
- sharing the real LAN endpoint only while listening;
- generating, revealing, copying, and regenerating session passwords;
- connecting to a trusted `host:port` address;
- managing local saved devices and session history; and
- configuring the active visual theme and host permissions.

Unsupported workflows are deliberately not represented as successful UI. File transfer has no desktop workflow yet and is not shown as a navigation destination.

## Architecture

```text
┌────────────────────────────────────────────────────────────────┐
│ Tauri desktop shell                                              │
│ React + TypeScript UI  ─────── Tauri commands/events             │
└────────────────────────────────────────────────────────────────┘
                                  │
┌─────────────────────────────────▼──────────────────────────────┐
│ Rust session core                                                │
│ SessionManager · SessionHandle · permissions · lifecycle         │
├────────────────────────────────────────────────────────────────┤
│ Protocol v5 · SPAKE2 authentication · encrypted transport       │
├────────────────────────────────────────────────────────────────┤
│ Screen capture · remote input · optional compression             │
└────────────────────────────────────────────────────────────────┘
```

The frontend is a control surface only. Connection setup, authentication, encryption, capture, and input remain in Rust; passwords and session keys are not exposed to frontend events or logs.

## Requirements

- Rust stable with Cargo
- Node.js 20 or newer and npm
- macOS for the currently validated desktop build path
- Platform requirements for capture/input backends when enabling `capture` and `input`

On macOS, Screen Recording permission is required for actual screen capture. Accessibility permission is required before the host can inject approved remote input. EasyControl does not bypass macOS security prompts.

## Quick start

### Rust core

```bash
git clone https://github.com/udhaybhat00/easycontrol.git
cd easycontrol
cargo check --all-features
cargo test --all-features
```

### Desktop application

```bash
cd desktop
npm ci
npm run tauri:dev
```

To create a macOS application bundle:

```bash
cd desktop
npm run tauri:build
```

The debug bundle is written to:

```text
desktop/src-tauri/target/debug/bundle/macos/EasyControl.app
```

## Direct connection workflow

1. On the host, open **Share this computer**.
2. Choose a listening port. EasyControl defaults to `5899` to avoid the conventional VNC/Screen Sharing port `5900`.
3. Start the host and share the displayed LAN address and session password through a trusted channel.
4. On the client, enter `host:port` and the session password, then choose **Connect**.
5. The host controls privileged permissions. A client cannot grant itself remote input.

This project currently has no cloud relay, public device identity, or discovery service. Direct connectivity depends on the network path between host and client.

## Development commands

| Task | Command |
| --- | --- |
| Format check | `cargo fmt --check` |
| Core type check | `cargo check --all-features` |
| Core tests | `cargo test --all-features` |
| Strict linting | `cargo clippy --all-features -- -D warnings` |
| Release core build | `cargo build --release --all-features` |
| Frontend build | `cd desktop && npm run build` |
| Desktop development | `cd desktop && npm run tauri:dev` |
| Desktop bundle | `cd desktop && npm run tauri:build` |

## QA application build

The QA configuration is development-only and uses a separate bundle identifier, application name, and browser-storage namespace. It lets a production host and isolated client run on one Mac without sharing the production app's local preferences.

```bash
cd desktop
VITE_EC_PROFILE=qa npm run tauri -- build --debug --config src-tauri/tauri.qa.conf.json
```

Do not distribute the QA build as a production artifact.

## Security model

- Password authentication uses SPAKE2.
- Session key confirmation is encrypted.
- Protocol version is v5.
- Remote input defaults to denied and is enforced by the host.
- The desktop UI does not save session passwords.
- Clipboard, chat, and file-transfer permissions are explicit session controls.
- The application does not provide an unauthenticated HTTP or WebSocket frame endpoint.

Security features require ongoing review. They should not be interpreted as a guarantee of suitability for high-risk or unattended deployments.

## Current limitations

- Real two-machine / two-instance macOS validation of capture, viewer rendering, input approval, revocation, reconnect behavior, and cleanup is still in progress.
- Performance telemetry for real sessions (FPS, dropped frames, CPU, memory, and throughput) has not been baselined.
- No cloud relay, NAT traversal, global device IDs, or device discovery.
- No complete desktop UI for clipboard, chat, or file transfer.
- macOS is the only desktop packaging target built in this repository so far; Windows and Linux are not yet validated releases.
- A DMG release workflow still needs end-to-end packaging validation.

## Contributing

Contributions are welcome. Please keep changes focused, preserve protocol compatibility unless explicitly justified, and run the relevant checks before opening a pull request.

For a UI change, run `cd desktop && npm run build`. For core changes, run the Rust validation commands listed above. Do not commit generated `target/`, `dist/`, or `node_modules/` artifacts.

## Project links

- [Source code and issues](https://github.com/udhaybhat00/easycontrol)
- [GitHub profile](https://github.com/udhaybhat00)
- [LinkedIn profile](https://in.linkedin.com/in/udhaybhat)

## License

No license file is currently included in this repository. Add an explicit license before publishing binaries or accepting third-party contributions under defined terms.
