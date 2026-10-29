# Contributing to EasyControl

Thanks for helping improve EasyControl. The application repository currently requires access; contact [Udhay Bhat](https://github.com/udhaybhat00) if you would like to contribute. The [website repository](https://github.com/udhaybhat00/easycontrol-website) is public.

## Before changing code

Read the [README](README.md) and [latest macOS QA report](docs/QA-2026-10-28.md). Preserve existing work, protocol v5 authentication, host-enforced permissions and bounded queues. Keep fixes focused; explain breaking changes explicitly.

For a bug report, include the revision, OS version, reproduction steps, expected behavior and observed result. Distinguish real OS behavior from mock-backend tests. Remove credentials, private addresses, captured screen content and clipboard contents from attachments.

## Development

Use Rust stable, Node.js 22.12+ and npm. Run `npm ci` inside `desktop/` before frontend work. Follow [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) for the host platform. macOS is the currently exercised desktop packaging path.

Run the relevant checks from the repository root:

```bash
cargo fmt --check
cargo check --all-features
cargo test --all-features
cargo clippy --all-features -- -D warnings
cargo build --release --all-features
cargo fmt --manifest-path desktop/src-tauri/Cargo.toml --check
cargo test --manifest-path desktop/src-tauri/Cargo.toml
cargo clippy --manifest-path desktop/src-tauri/Cargo.toml -- -D warnings
```

Then inside `desktop/`:

```bash
npm test
npm run build
```

For website changes, run `npm test` and `npm run build` inside `website/`, inspect desktop and narrow layouts, and check keyboard navigation and both themes.

## Pull requests

- Describe the problem and the resulting behavior.
- Include meaningful tests for behavior changes and report the commands actually run.
- Label mocked, real OS and unperformed checks accurately.
- Keep generated `target/`, `dist/`, `node_modules/`, app bundles, signing credentials and OS metadata out of the diff.
- Do not weaken authentication or add broad Tauri capabilities for convenience.

Contributions are provided under the repository’s [MIT License](LICENSE).
