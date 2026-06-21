---
scope: L2
summary: "How the desktop auto-updater works and the one-time signing-key setup required to activate it"
modified: 2026-06-21
reviewed: 2026-06-21
lifecycle: ephemeral
type: HOWTO
depends:
  - path: docs/eph/REPORT-L2-public-beta-readiness-audit
---

# Desktop auto-update

The desktop app checks for updates on launch and offers a one-click
"Install & restart". Updates are served from the GitHub Releases `latest.json`
manifest and verified against a bundled public key before install.

## How it works

- **Client**: `apps/desktop` registers `tauri-plugin-updater` and
  `tauri-plugin-process`. The frontend hook `useDesktopUpdates` (run only in the
  main Tauri window) calls the updater on startup; if an update is available it
  shows a toast with an install action that downloads, installs, and relaunches.
  The browser-localhost build and secondary surface windows are no-ops.
- **Config**: `apps/desktop/tauri.conf.json` `plugins.updater` holds the public
  key and the manifest endpoint
  (`.../releases/latest/download/latest.json`). The matching capability
  permissions are `updater:default` and `process:allow-restart`.
- **Release**: the `release.yml` desktop build emits signed updater artifacts
  (`.sig`, plus the macOS `.app.tar.gz`) **only when the signing key is present**
  (`createUpdaterArtifacts` is injected via `--config`, not hardcoded, so
  releases still build without it). The publish job runs
  `tools/release/generate-updater-manifest.sh` to assemble `latest.json` from the
  collected `.sig` files and uploads it with the release.

## One-time activation (required)

The public key is committed; the **private** key is the secret. Until the
secrets below are set, releases build normally but produce no updater artifacts
and no `latest.json`, so installed apps simply never see an update.

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
4. Cut a release as usual. Verify the release contains `latest.json` and the
   per-platform `.sig` (and macOS `.app.tar.gz`) assets.

## Scope and limitations

- macOS auto-update covers the arm64 (`darwin-aarch64`) build only, matching the
  single-arch dmg currently distributed. Intel macOS users update manually.
- Updates are check-on-launch; there is no periodic background check yet.
- Rotating the key requires shipping a new public key in `tauri.conf.json`;
  clients on the old key cannot verify updates signed by a new key.
