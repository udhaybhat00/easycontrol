# Repository and website maintenance

As of 31 October 2026, `udhaybhat00/easycontrol` is the single maintained repository for the application, website, documentation, screenshots and project credits.

## Layout and links

- `README.md`: project entry point, images, architecture, setup and current release limitations.
- `website/`: complete static product site and searchable usage guide.
- `desktop/`: Tauri application; About links to this repository, its issues and usage documentation, plus the creator's GitHub and LinkedIn profiles.
- `.github/workflows/website.yml`: website tests/build from `website/`; optional deployment uploads only `website/dist/`.
- `docs/QA-2026-10-28.md` and `docs/HARDENING-2026-10-30.md`: dated QA evidence. Consolidation does not change their recorded test results.

The former `easycontrol-website` repository's tracked files matched `website/` before consolidation. Its complete Git history was backed up locally before removal. Future edits belong here; do not recreate a second synchronized website repository.

## Publication boundary

On 31 October 2026, the owner completed deletion of `udhaybhat00/easycontrol-website`, then explicitly approved making `udhaybhat00/easycontrol` public and enabling public website hosting. GitHub confirms the retained repository is public and Pages is configured with GitHub Actions at https://udhaybhat00.github.io/easycontrol/.

The earlier publication block applied while the repository was private and publication was not approved. It is resolved by the owner's later explicit authorization. No billing or spending setting was changed.

The Actions repository variable `PAGES_ENABLED=true` enables the root website workflow. It tests and builds on relevant pushes and pull requests, and deploys only `website/dist/` from `master` outside pull requests. Check the workflow run to identify the latest successfully deployed revision. Application source is public on GitHub, but is not included in the static hosting artifact.

Source releases are development prereleases. Public availability does not satisfy the remaining real-input, platform, signing, notarization or security-review gates documented in the QA reports.

For local preview, run `npm test`, `npm run build`, then `npm run dev` from `website/`. The site serves at `http://127.0.0.1:4173`. Never publish development logs, application data or signing credentials as static content.
