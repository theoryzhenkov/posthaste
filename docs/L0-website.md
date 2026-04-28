---
scope: L0
summary: "Public PostHaste product showcase site, visual direction, and container deployment"
modified: 2026-04-25
reviewed: 2026-04-25
depends:
  - path: README
  - path: docs/L0-branding
  - path: docs/L0-ui
dependents: []
---

# Website -- L0

## Purpose

The public website showcases PostHaste as a product without replacing the mail client application. It lives in `apps/site/` as a static Astro site, separate from the production client in `apps/web/`.

The site should show the interface rather than argue for it. Product mockups, mailbox color, search, smart mailbox flow, and theme surfaces are primary. Copy stays short and project-like.

Home page copy should be editable as Markdown under `apps/site/src/content/home/`. Placeholder lorem ipsum may remain until final copy is curated, but current deliberate non-placeholder copy includes the title-page slogan, the `Welcome` mock email, the `Community extensions` mock email, and the `Open Source` landscape strip.

## Visual Direction

The website inherits the product's shell language from [L0-branding](L0-branding.md) and [L0-ui](L0-ui.md). The first viewport should feel like the PostHaste client itself: dark neutral surface, toolbar, sidebar, message list, reader pane, thin separators, and compact typography.

- Dark natural background, not a single saturated brand wash.
- Multiple semantic accent colors for mailboxes, tags, unread state, flag state, and theme previews.
- Glass treatment reserved for theme and overlay moments, not the base page.
- Product UI mockups should use the client shell as the main website structure.
- The title-page reader pane should present `Your Mail Delivered at PostHaste` as the main slogan, with a small line gap between `Your Mail` and `Delivered at PostHaste`.
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

## Deployment

The Docker image is built from `apps/site/Dockerfile`. The build stage runs `bun run build`; the runtime stage serves `dist/` through Nginx on port `80`.

Expected local commands:

```sh
just site dev
just site build
docker build -t posthaste-site apps/site
```

## Invariants

- `apps/site/` remains separate from `apps/web/`.
- The public site is static and has no dependency on the mail daemon.
- Logo and favicon assets must stay usable on dark backgrounds.
- The first viewport must show the PostHaste name and product interface direction.
- Theme/glass treatment must not become the dominant style of the entire site.
