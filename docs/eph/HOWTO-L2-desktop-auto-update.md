---
scope: L2
summary: "How the desktop auto-updater works and the one-time signing-key setup required to activate it"
modified: 2026-06-25
reviewed: 2026-06-25
lifecycle: ephemeral
type: HOWTO
depends:
  - path: docs/eph/REPORT-L2-public-beta-readiness-audit
---

# Desktop auto-update

The desktop app checks for updates on launch and offers a one-click
"Install & restart". Updates are served from the channel's GitHub Releases
manifest (`latest.json` for nightly, `latest-stable.json` for stable) and
verified against a bundled public key before install.

## How it works

- **Client**: `apps/desktop` registers `tauri-plugin-updater` and
  `tauri-plugin-process`. The frontend hook `useDesktopUpdates` (run only in the
  main Tauri window) calls the updater on startup; if an update is available it
  shows a toast with an install action that downloads, installs, and relaunches.
  The browser-localhost build and secondary surface windows are no-ops.
- **Config**: `apps/desktop/tauri.conf.json` `plugins.updater` holds the public
  key and a default manifest endpoint
  (`.../releases/latest/download/latest.json`) for unsupervised local builds.
  The release workflow overrides the endpoint at build time via `--config` so
  that nightly artifacts follow the `nightly` rolling tag and stable artifacts
  follow the `stable` rolling tag.
- **Release**: the `release.yml` desktop build emits signed updater artifacts
  (`.sig`, plus the macOS `.app.tar.gz`) **only when the signing key is present**
  (`createUpdaterArtifacts` is injected via `--config`, not hardcoded, so
  releases still build without it). The manifest file is now channel-scoped:
  `latest.json` for nightly builds and `latest-stable.json` for stable builds.
  The publish job uses `tools/release/generate-updater-manifest.sh` with the
  correct output filename and then updates the `nightly` or `stable` rolling tag
  so each channel has a stable URL. Linux/Windows builds still must not pass
  `tauri build --no-sign`, because that flag also skips *updater* signing and
  would silently drop those platforms from the manifest.

## One-time activation (required)

The public key is committed; the **private** key is the secret. Until the
secrets below are set, releases build normally but produce no updater artifacts
and no channel manifest, so installed apps simply never see an update.

1. The generated private key lives **outside the repo** at
   `~/.secrets/posthaste-updater.key` on the dev VM (mode 600), and its password
   at `~/.secrets/posthaste-updater.key.password` (mode 600). The key is
   password-protected because GitHub Actions rejects empty secret values.
2. Add two GitHub Actions repository secrets:
   - `TAURI_SIGNING_PRIVATE_KEY` — the full contents of
     `~/.secrets/posthaste-updater.key`.
   - `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` — the contents of
     `~/.secrets/posthaste-updater.key.password`.
3. After adding the secrets, delete the local private-key and password files
   once they are safely stored in GitHub secrets (and a personal backup).
4. Cut a release as usual. Verify the release contains the channel's manifest
   (`latest.json` for nightly, `latest-stable.json` for stable), the per-platform
   `.sig` files (and macOS `.app.tar.gz`), and that the channel's rolling tag
   (`nightly` or `stable`) points to it.

## Manual check

Settings → General → Updates has a **Check for updates** button (desktop only).
It reuses the same helpers as the launch check (`src/desktopUpdates.ts`) and
reports "up to date" explicitly. The launch check is silent unless an update is
found.

## Channels

Releases are split into two channels by tag pattern:

- **Nightly**: `vX.Y.Z-dogfood.N` or any `vX.Y.Z-nightly.*`. These builds include
  the `devtools` feature, can fall back to ad-hoc macOS signing when opted in,
  and write `latest.json`.
- **Stable**: `vX.Y.Z-beta.N`, `vX.Y.Z-rc.N`, or plain `vX.Y.Z`. These builds
  omit `devtools`, require Developer ID + notarization on macOS, and write
  `latest-stable.json`.

The app gets its endpoint at compile time, so a single artifact cannot switch
channels. GitHub's `releases/latest/download/` URL is intentionally avoided:
instead, CI maintains lightweight `nightly` and `stable` tags that always point
at the latest release in each channel. The static asset URLs are:

```text
nightly: https://github.com/theoryzhenkov/posthaste/releases/download/nightly/latest.json
stable:  https://github.com/theoryzhenkov/posthaste/releases/download/stable/latest-stable.json
```

Only stable releases are marked `make_latest` on GitHub, so the public releases
page highlights the stable channel while the rolling tags keep updater traffic
separated.

See `docs/eph/DESIGN-L2-release-channels.md` for the full channel design,
version mapping, and the workflow shape.

## Scope and limitations

- macOS auto-update covers the arm64 (`darwin-aarch64`) build only, matching the
  single-arch dmg currently distributed. Intel macOS users update manually.
- Updates are check-on-launch plus the manual button; no periodic background
  check yet.
- Rotating the key requires shipping a new public key in `tauri.conf.json`;
  clients on the old key cannot verify updates signed by a new key.
