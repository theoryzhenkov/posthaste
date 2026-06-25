---
scope: L2
summary: "Release-channel split: nightly (dogfood/devtools) versus stable (public beta/release), driven by tag pattern, with separate updater manifests, rolling-tag updater URLs, macOS signing policy, and artifact smoke gates."
modified: 2026-06-25
reviewed: 2026-06-25
lifecycle: ephemeral
type: DESIGN
depends:
  - path: .github/workflows/release
  - path: tools/release/generate-updater-manifest
  - path: apps/desktop/tauri.conf
  - path: tools/release/smoke-desktop-bundle
  - path: tools/release/resolve-channel
  - path: tools/release/update-rolling-tag
dependents: []
---

# Release channel design

## The problem

Posthaste currently ships every desktop release from the same tag shape
(`v0.1.0-dogfood.N`) to the same updater manifest (`latest.json`). The single
artifact includes the embedded authority runtime **and** the Tauri DevTools
feature, gated at runtime. That worked for dogfood, but it does not work for a
public beta because:

- Public-beta users should not be offered dogfood builds, and dogfood users
  should not be pulled onto a newer stable build before it is ready.
- DevTools are acceptable for internal dogfood but must not ship in public
  installers.
- macOS dogfood builds are signed with Developer ID when secrets are present,
  with an ad-hoc opt-out. Public-beta macOS builds must be fail-closed signed +
  notarized.
- The auto-updater has one `latest.json`; a channel split needs at least two
  manifests.

## Channels

```text
┌─────────┬────────────────┬──────────────────────────────┬───────────────────────┐
│ Channel │ Audience       │ Tag pattern                  │ Desktop build flags   │
├─────────┼────────────────┼──────────────────────────────┼───────────────────────┤
│ nightly │ Dogfood / dev  │ vX.Y.Z-dogfood.N             │ embedded-server +     │
│         │                │ vX.Y.Z-nightly.*             │ devtools              │
├─────────┼────────────────┼──────────────────────────────┼───────────────────────┤
│ stable  │ Public beta /  │ vX.Y.Z-beta.N                │ embedded-server only, │
│         │ release        │ vX.Y.Z-rc.N                  │ no devtools           │
│         │                │ vX.Y.Z (plain)               │                       │
└─────────┴────────────────┴──────────────────────────────┴───────────────────────┘
```

### Updater manifest assignment

- `nightly` → `latest.json`.
- `stable` → `latest-stable.json`.

Each manifest only contains releases from its own channel. The manifest files
are attached to every GitHub Release, but the app does **not** follow GitHub's
`releases/latest/download/` URL because that URL would switch between channels
whenever a different channel was published. Instead, each channel uses a rolling
git tag:

- `nightly` tag — always points to the latest nightly release.
- `stable` tag — always points to the latest stable release.

The baked-in updater endpoints are:

```text
nightly: https://github.com/theoryzhenkov/posthaste/releases/download/nightly/latest.json
stable:  https://github.com/theoryzhenkov/posthaste/releases/download/stable/latest-stable.json
```

The publish job force-updates the rolling tag for the current channel after the
release is created, so a channel's static URL always serves its latest
manifest.

### macOS signing policy

- **Nightly**: Developer ID if secrets are present; ad-hoc if
  `POSTHASTE_MACOS_SIGNING=adhoc` is explicitly requested. This preserves the
  current opt-out used for CI forks and unsigned internal builds.
- **Stable**: Fail-closed Developer ID **plus** notarization credentials. The
  workflow refuses to publish a stable macOS build that is not both signed and
  notarized.

### Artifact smoke

- **Nightly**: light smoke — bundle files exist, can be extracted/listed, and
  basic structure is valid.
- **Stable**: full smoke — AppImage is extracted and the bundled binary answers
  `--version` and `--help`; stable binaries/ assets must not contain dev-server
  endpoint strings or devtools-related artifacts.

## How the app knows its channel

Tauri reads the updater endpoint list from `tauri.conf.json`. The checked-in
config uses `.../releases/latest/download/latest.json` as the default so local
developer builds follow the existing dogfood/nightly path. The release workflow
overrides the endpoint at build time with `--config`:

- `nightly` is rewritten to the `nightly` rolling-tag URL.
- `stable` is rewritten to the `stable` rolling-tag URL.

This is compile-time binding: the produced artifact cannot switch manifests.

`apps/desktop/tauri.conf.json` must keep the default endpoint literal; do not
make it environment-driven. Each channel build receives the correct literal
via the Tauri CLI `--config` merge.

## Version mapping

macOS `CFBundleShortVersionString` accepts only three non-negative integers. The
following tag-to-version mapping is used for both the embedded app version and
the updater manifest:

| Tag                         | App / manifest version |
| --------------------------- | ---------------------- |
| `vA.B.C-dogfood.N`          | `A.B.N`                |
| `vA.B.C-beta.N`             | `A.B.N`                |
| `vA.B.C-rc.N`               | `A.B.N`                |
| `vA.B.C` (plain stable)     | `A.B.C`                |

This means a `v0.2.0-beta.5` build embeds version `0.2.5`. A following plain
`v0.2.0` release embeds `0.2.0`, which semver considers **older** than `0.2.5`.
Installed beta users would not auto-update to the plain release. To avoid this,
a release that follows a beta/rc cycle must use a numerically higher version
(e.g. `v0.2.1` or `v0.3.0`). This convention is enforced by reviewer check,
not by CI, because CI cannot know future tag intent.

## Workflow changes

1. **Add a `resolve-channel` job** that runs first and exports:
   - `POSTHASTE_RELEASE_CHANNEL` (`nightly` | `stable`)
   - `include_devtools` (`true` | `false`)
   - `enforce_macos_signing` (`true` | `false`)
   - `run_artifact_smoke` (`true` | `false`)
   - `updater_manifest_filename` (`latest.json` | `latest-stable.json`)
   - `is_stable` (`true` | `false`)

2. **`build-desktop` consumes those outputs**:
   - Conditionally pass `--features devtools`.
   - Override `tauri.conf.json` updater `endpoints` via `--config` to the
     channel's rolling-tag URL.
   - For macOS, fail the job early on stable if Developer ID and notarization
     secrets are not all present.

3. **Desktop version derivation** is moved into a small shell script so the
   inline YAML and `generate-updater-manifest.sh` use the same transform.

4. **Add a smoke step** to the desktop build job after bundle collection and
   before artifact upload. The step runs
   `tools/release/smoke-desktop-bundle.sh <channel> <platform> <bundle-dir>`
   and exits non-zero on failure.

5. **`generate-updater-manifest.sh` accepts an output filename argument**
   instead of hard-coding `latest.json`. The publish job passes
   `latest-stable.json` for stable tags and `latest.json` for nightly tags.

6. **`update-rolling-tag.sh`** updates the `nightly` or `stable` lightweight
   tag to the current release tag after publish.

7. **Publish job** force-updates the rolling `nightly` or `stable` tag to the
   current release after creation, so each channel's static updater URL always
   points to its latest manifest. It also sets `make_latest` only for stable
   releases, so GitHub's release page highlights the public channel while the
   rolling tags keep updater traffic separated.

## Assertions

| ID                        | Sev.   | Assertion                                                                                  |
| ------------------------- | ------ | ------------------------------------------------------------------------------------------ |
| tag-detection             | MUST   | Every release tag resolves unambiguously to `nightly` or `stable`; unknown tags fail CI.     |
| nightly-devtools          | MUST   | Nightly desktop builds compile with the `devtools` feature.                                  |
| stable-no-devtools        | MUST   | Stable desktop builds do not compile with the `devtools` feature.                          |
| channel-rolling-tag       | MUST   | The publish job updates the `nightly` or `stable` rolling tag to the current release for the current channel. |
| channel-updater-endpoint  | MUST   | Each built desktop artifact has its channel's rolling-tag updater endpoint baked in at compile time.     |
| stable-manifest-name      | MUST   | Stable releases publish `latest-stable.json`; nightly releases publish `latest.json`.      |
| macos-stable-signing      | MUST   | Stable macOS builds require Developer ID signing and notarization credentials.             |
| macos-nightly-signing     | SHOULD | Nightly macOS builds sign with Developer ID when secrets are present or adhoc when opted in. |
| version-three-integers    | MUST   | The app/manifest version derived from any tag is a valid 3-integer semver for macOS.       |
| stable-artifact-smoke     | MUST   | Stable desktop artifacts pass full smoke (extract, version/help, no dev strings).        |
| nightly-artifact-smoke    | SHOULD | Nightly desktop artifacts pass light smoke before upload.                                  |

## Code anchors

- Release workflow: `.github/workflows/release.yml`
- Tauri config: `apps/desktop/tauri.conf.json`
- Updater manifest script: `tools/release/generate-updater-manifest.sh`
- Channel resolver: `tools/release/resolve-channel.sh`
- Bundle version helper: `tools/release/bundle-version-from-tag.sh`
- Bundle smoke: `tools/release/smoke-desktop-bundle.sh`
- Rolling tag helper: `tools/release/update-rolling-tag.sh`
