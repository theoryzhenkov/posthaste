---
scope: L0
summary: "Public PostHaste product showcase site, visual direction, and container deployment"
modified: 2026-06-01
reviewed: 2026-06-01
depends:
  - path: README
  - path: docs/L0-branding
  - path: docs/L0-ui
dependents: []
---

# Website -- L0

## Purpose

The public website showcases PostHaste as a product without replacing the mail client application. It lives in `apps/site/` as a static Astro site, separate from the production client in `apps/web/`.

The site should show the interface rather than argue for it. Product mockups, mailbox color, smart mailbox flow, local/API surfaces, and theme surfaces are primary. Copy stays short and project-like.

Home page copy should be editable as Markdown under `apps/site/src/content/home/`. Public copy should be confident, short, and grounded in implemented or explicitly planned product surfaces. It should include one clear early-build caveat without making every section apologize for the project's stage. Current deliberate non-placeholder copy includes the title-page slogan, product-direction `Welcome` mock email, builder/API mock email, search/local-first mock emails, notes section, interface section, footer, and open-source landscape strip.

## Visual Direction

The website inherits the product's shell language from [L0-branding](L0-branding.md) and [L0-ui](L0-ui.md). The first viewport should feel like the PostHaste client itself: dark neutral surface, toolbar, sidebar, message list, reader pane, thin separators, and compact typography.

- Dark natural background, not a single saturated brand wash.
- Multiple semantic accent colors for mailboxes, tags, unread state, flag state, and theme previews.
- Glass treatment reserved for theme and overlay moments, not the base page.
- Product UI mockups should use the client shell as the main website structure.
- The title-page reader pane should present `Your mail, delivered to you at Posthaste` as the main slogan, with a small line gap between `Your mail,` and `delivered to you at Posthaste`.
- Mock email rows in the title-page shell should be selectable and update the reader pane.
- The title-page shell should keep outer page padding, but sit directly on the dark site background without an outer framed-card border or drop shadow.
- Install should be exposed through a fixed floating command strip at the top of the page. The strip should keep page-edge padding, include a drag handle, an active pin button, a direct install button, and compact navigation links. The title-page shell should start lower so the strip does not obscure the mail UI on first load.
- Subtle scroll reveals only; motion must stay unintrusive and respect `prefers-reduced-motion`.

The logo asset uses the compact PostHaste P/arrow mark. The lighter landscape/priorities section uses the mark directly inside a layered palette landscape that echoes the favicon without the favicon's rounded-square backing. The landscape animation should be implemented as one long duplicated scenery strip that pans past like a train-window panorama, not as independent elements that reset separately. Terrain paths must be periodic at the strip boundary: matching height and tangent at the beginning and end, plus a slight rendered overlap between duplicated strips to hide antialiasing. Looped terrain fills should be opaque so the overlap does not create darker stripes; translucent texture should come from a static wash over the full scene instead. Landscape SVG tokens may live under `apps/site/public/assets/landscape/` once a polished asset set exists, but the current strip intentionally uses only terrain layers. The loop should run long enough to feel like a slow panorama, roughly one to two minutes before repeating, while the terrain itself should vary often enough that the motion does not feel static. It should keep one sun, avoid road-like brown foregrounds, and disable motion under `prefers-reduced-motion`. The favicon uses the same mark in light cream on a playful multi-color palette background so it remains legible in browser tabs and in the dark titlebar.

The landscape should reflect the viewer's local browser time. Morning, day, evening, and night states may change the sky and terrain palette. The sun position should roughly follow the daytime clock arc; at night, the scene may switch to a moon/night treatment.

## Architecture

`apps/site/` is a static frontend:

- Astro + TypeScript.
- React is used for the interactive home page island that contains the fixed install strip, selectable mail mock, timed landscape, reveal behavior, and theme preview.
- Home page copy is loaded from Markdown files under `apps/site/src/content/home/` at build time and passed into the React island as typed content.
- CSS is local to the site and does not import the mail client app CSS.
- Assets live under `apps/site/public/`.
- The production container builds static assets and serves them with Nginx.

The site must not call the local PostHaste API or JMAP. It is deployable independently from the desktop/web client stack.

## Releases and Downloads

The site serves a static `/releases` downloads-and-changelog page, rendered at build time (no client-side data fetch).

- Each release is a Markdown file under `apps/site/src/content/releases/<version>.md`. Frontmatter carries the structured data (version, tag, date, prerelease flag, per-platform download assets, checksum/GPG-key URLs); the Markdown body holds optional hand-authored dev notes for that version.
- These files are the source of truth. The releases page (`src/pages/releases.astro` + the `Releases` React island) reads the whole collection through `releasesContent.ts`, which globs the directory so newly added files need no loader changes.
- `apps/site/tools/generate-release-notes.mjs` writes a release's frontmatter from its GitHub release (mapping tag `v0.1.0-dogfood.N` to version `0.1.N`, matching the desktop version mapping). It preserves any existing notes body, so regenerating never clobbers hand-written notes. Asset classification keeps the desktop installers (`.dmg`, `.exe`, `.msi`, `.AppImage`, `.deb`, `.rpm`) and drops signatures, checksums, the self-host server bundle, and retired build variants.
- The download grid auto-detects the visitor's OS after hydration to highlight their platform; with JavaScript off, all platforms are shown equally.

A new release flows to the live page automatically: `release-notes.yml` regenerates and commits the content file on `release: published`, which lands on `main` and retriggers the site image build.

## Deployment

The Docker image is built from `apps/site/Dockerfile`. The build stage installs the Bun workspace (all member manifests are copied so `--frozen-lockfile` resolves) and runs `bun run build`; the runtime stage serves `dist/` through Nginx on port `80`.

The site is published and deployed continuously:

- `.github/workflows/site-publish.yml` builds and pushes `ghcr.io/theoryzhenkov/posthaste/posthaste-site:latest` on every push to `main` that touches `apps/site/**`.
- The image is deployed on the `tars` host through the ops_atlas app framework: `hosts/nixos/tars/apps/posthaste-theor-net/app.yaml` declares the container (port `80`, host port `8104`, domain `posthaste.theor.net`), which nginx serves with ACME TLS. The `docker-auto-update` timer pulls `:latest` every ~5 minutes, so merges to `main` roll out without a manual deploy.
- Public DNS for `posthaste.theor.net` is a Porkbun A-record in the ops_atlas DNS Terraform root, pointing at the tars public IP.

Expected local commands:

```sh
just site dev
just site build
docker build -f apps/site/Dockerfile -t posthaste-site .   # build context is the repo root
```

## Invariants

- `apps/site/` remains separate from `apps/web/`.
- The public site is static and has no dependency on the mail daemon.
- Logo and favicon assets must stay usable on dark backgrounds.
- The first viewport must show the PostHaste name and product interface direction.
- Theme/glass treatment must not become the dominant style of the entire site.
