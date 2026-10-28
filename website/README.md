# EasyControl website

The public product website and documentation for EasyControl, a Rust + Tauri remote-desktop application. This repository contains **only website source and reviewed application screenshots**. It does not contain the private application source, session credentials, captured remote screens or signing material.

**Live site:** https://udhaybhat00.github.io/easycontrol-website/

## Develop

Requires Node.js 20+. No npm dependencies are installed. Python 3 is used only for the local preview server.

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
└── .github/workflows/   GitHub Pages build and deployment
```

## Deployment

GitHub Pages uses GitHub Actions. The workflow tests and builds this static site, uploads only `dist/` and deploys with `actions/deploy-pages`. It runs on pushes to `main` and can be started manually. In repository Settings → Pages, select **GitHub Actions** as the build source.

The authoritative development copy also lives in `website/` inside the application repository. To publish an update, copy the reviewed website files into this website-only checkout, run the checks, then commit and push. Do not copy app source, QA logs, credentials or private remote screenshots into this public repository.

## Design and accessibility

Native system fonts, graphite surfaces and lavender accents. System/Light/Dark appearance is stored locally under a website-specific key. Keyboard-accessible screenshot tabs, native FAQ disclosures, visible focus rings, skip navigation, responsive menus with Escape dismissal and reduced-motion handling are included. All app images are actual idle-state screenshots, not simulated product sessions.

The site loads no third-party fonts, analytics, trackers or runtime dependencies. The site does not initiate desktop connections.

## Product status

EasyControl is development software. The website explicitly distinguishes observed macOS behavior from automated/mock coverage and untested release gates. The application repository currently requires access. There is no public signed installer release yet.

## Credits and license

Created by [Udhay Bhat](https://github.com/udhaybhat00) · [LinkedIn](https://in.linkedin.com/in/udhaybhat).

[MIT License](LICENSE). Copyright © 2026 Udhay Bhat.
