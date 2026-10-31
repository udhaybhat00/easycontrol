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

The retained repository remains private. The former website URL is scheduled for retirement. The owner confirmed deletion. Manual completion is pending: the CLI token lacks `delete_repo` permission and browser automation fails with `AXError.invalidUIElement` at the confirmation button. `https://udhaybhat00.github.io/easycontrol/` is the intended canonical URL, not a verified live deployment.

Pages activation is not part of this consolidation: automatic approval review rejected enabling it because publication could expose content from a private repository. A future deployment needs explicit owner approval for public website publication and a GitHub plan supporting Pages from this private repository. No source visibility, billing, or spending setting was changed.

Once approved and supported, select GitHub Actions in Settings → Pages and set the Actions repository variable `PAGES_ENABLED=true`. The workflow builds on relevant pushes and pull requests, but only deploys `master` outside pull requests. Leave the variable unset until publishing is approved. A private repository does not necessarily imply a private published website.

For local preview, run `npm test`, `npm run build`, then `npm run dev` from `website/`. The site serves at `http://127.0.0.1:4173`. Never publish the repository root, development logs, application data or signing credentials as static content.
