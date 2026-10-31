# EasyControl website

The product website and documentation for EasyControl, maintained alongside the Rust engine and Tauri desktop app in **udhaybhat00/easycontrol**. This directory is the only maintained website source. The former website-only repository was deleted by the owner on 31 October 2026; its old Pages deployment is retired.

**Public website:** https://udhaybhat00.github.io/easycontrol/ · [Documentation](https://udhaybhat00.github.io/easycontrol/docs.html). The application, site and documentation source are public in the same repository.

## Develop

Requires Node.js 22.12+. No npm dependencies are installed. Python 3 is used only for the local preview server.

```bash
npm run build
npm test
npm run dev
```

Open http://127.0.0.1:4173. The build outputs `dist/`, which is ignored by Git. Change the canonical URL in `site.config.json` when hosting elsewhere, or set `SITE_URL` for a build. All document/asset navigation is relative and supports GitHub Pages project subpaths.

## Structure

```text
.
├── index.html           Product page
├── docs.html            Searchable build and usage documentation
├── 404.html             Project-aware not-found page
├── styles.css           Shared responsive dark/light design system
├── app.js               Theme, navigation, tabs, copy and search
├── assets/              Real stopped-dashboard/settings screenshots + logo
├── build.mjs            Dependency-free static build
├── tests/               Build, links, anchors and privacy checks
├── site.config.json     Canonical website configuration
└── ../.github/workflows/website.yml   Root website workflow
```

## Deployment

The root workflow tests and builds this static site on relevant pushes to `master` and pull requests, and can be started manually. The owner approved public source and website publication on 31 October 2026. Pages uses GitHub Actions and `PAGES_ENABLED=true`; deployment runs only on `master` outside pull requests. It uploads only `website/dist/`, never application source, QA logs or signing material.

This repository and the deployed website are public. Check the workflow result before claiming a particular revision is deployed. For another static host, set `SITE_URL` during the build and publish only `dist/`. No cross-repository copying is needed.

## Design and accessibility

Native system fonts, graphite surfaces and lavender accents. System/Light/Dark appearance is stored locally under a website-specific key. Keyboard-accessible screenshot tabs, native FAQ disclosures, visible focus rings, skip navigation, responsive menus with Escape dismissal and reduced-motion handling are included. All app images are actual idle-state screenshots, not simulated product sessions.

The site loads no third-party fonts, analytics, trackers or runtime dependencies. The site does not initiate desktop connections.

## Product status

EasyControl is development software. The website distinguishes observed macOS behavior from automated/mock coverage and untested release gates. Source is publicly available under MIT. There is no public distribution-ready installer release yet. See the [latest hardening report](../docs/HARDENING-2026-10-30.md).

## Credits and license

Created by [Udhay Bhat](https://github.com/udhaybhat00) · [LinkedIn](https://in.linkedin.com/in/udhaybhat).

[MIT License](../LICENSE). Copyright © 2026 Udhay Bhat.
